//! Bundled-app boundary for the pre-runtime restore owner and OS file selection.
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tauri::{Manager, WebviewWindow};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tauri::command]
pub(crate) async fn server_restore(
    window: WebviewWindow,
    mut request: Value,
) -> Result<Value, String> {
    crate::caller_is_bundled_ui(&window)?;
    let action = request
        .get("action")
        .and_then(Value::as_str)
        .ok_or("복원 요청이 올바르지 않아요.")?;
    if action == "prepare" {
        if request.get("file").is_some() {
            return Err("백업 파일은 파일 선택 창에서 선택해 주세요.".into());
        }
        let selected = rfd::AsyncFileDialog::new()
            .add_filter("AgentsAssemble 서버 백업", &["aaserver"])
            .pick_file()
            .await;
        let Some(selected) = selected else {
            return Ok(json!({"cancelled":true}));
        };
        request["file"] = json!(selected.path());
    }
    let app_data = window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|_| "앱 데이터 위치를 찾지 못했어요.")?;
    std::fs::create_dir_all(&app_data).map_err(|_| "앱 데이터 폴더를 만들지 못했어요.")?;
    crate::local_runtime::make_private_directory(&app_data)
        .map_err(|_| "앱 데이터 폴더를 보호하지 못했어요.")?;
    let database = app_data.join("runtime.sqlite3");
    let desktop = std::env::current_exe().map_err(|_| "앱 실행 경로를 찾지 못했어요.")?;
    let executable = crate::local_runtime::sidecar_executable(&desktop)?;
    let bytes = serde_json::to_vec(&request).map_err(|_| "복원 요청이 올바르지 않아요.")?;
    if bytes.len() > 16384 {
        return Err("복원 요청이 너무 커요.".into());
    }
    let mut command = tokio::process::Command::new(executable);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .arg("--server-restore")
        .arg(database)
        .env(
            "AGENTSASSEMBLE_CENTRAL_URL",
            env!("AGENTSASSEMBLE_CENTRAL_URL"),
        )
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "복원 작업을 시작하지 못했어요.")?;
    let mut stdin = child.stdin.take().ok_or("복원 입력을 열지 못했어요.")?;
    let stdout = child.stdout.take().ok_or("복원 출력을 열지 못했어요.")?;
    let result = tokio::time::timeout(Duration::from_mins(10), async {
        stdin.write_all(&bytes).await?;
        drop(stdin);
        let mut output = Vec::new();
        stdout.take(32769).read_to_end(&mut output).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((status, output))
    })
    .await;
    match result {
        Ok(Ok((status, output))) if status.success() && output.len() <= 32768 => {
            let response: Value =
                serde_json::from_slice(&output).map_err(|_| "복원 응답이 올바르지 않아요.")?;
            if response.get("ok").and_then(Value::as_bool) != Some(true) {
                return Err("복원 작업을 완료하지 못했어요. 백업 파일·암호·로그인 상태를 확인하고 작업 상태를 다시 확인해 주세요.".into());
            }
            Ok(response["result"].clone())
        }
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err("복원 응답을 확인하지 못했어요. 작업 상태를 다시 확인해 주세요.".into())
        }
    }
}
