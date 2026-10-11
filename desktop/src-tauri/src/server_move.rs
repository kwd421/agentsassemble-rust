//! Bundled-app boundary for the pre-runtime restore owner and OS file selection.
use serde_json::{Value, json};
use std::{process::Stdio, time::Duration};
use tauri::{Manager, WebviewWindow};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
pub(crate) struct SelectedBackup(std::sync::Mutex<Option<std::path::PathBuf>>);

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
    if request.get("file").is_some() {
        return Err("백업 파일은 파일 선택 창에서 선택해 주세요.".into());
    }
    if action == "select" {
        let selected = rfd::AsyncFileDialog::new()
            .add_filter("AgentsAssemble 서버 백업", &["aaserver"])
            .pick_file()
            .await;
        let Some(selected) = selected else {
            return Ok(json!({"cancelled":true}));
        };
        let path = selected.path().to_owned();
        *window
            .state::<SelectedBackup>()
            .0
            .lock()
            .map_err(|_| "파일 선택 상태를 확인하지 못했어요.")? = Some(path.clone());
        request = json!({"action":"probe","file":path});
    } else if action == "prepare" {
        let path = window
            .state::<SelectedBackup>()
            .0
            .lock()
            .map_err(|_| "파일 선택 상태를 확인하지 못했어요.")?
            .clone()
            .ok_or("백업 파일을 먼저 선택해 주세요.")?;
        request["file"] = json!(path);
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
                return Err(restore_error(&response).into());
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

fn restore_error(response: &Value) -> &'static str {
    match response.get("error").and_then(Value::as_str) {
        Some("restore_target_not_empty") => {
            "이 기기에 이미 서버 데이터가 있어요. 데이터가 없는 새 기기에서 가져와 주세요."
        }
        Some("restore_pending") => {
            "진행 중인 복원 작업이 있어요. 작업 상태를 확인하고 재개해 주세요."
        }
        Some("backup_authentication_failed") => {
            "비밀번호가 맞지 않거나 백업 파일이 손상됐어요. 비밀번호를 확인해 주세요."
        }
        Some("backup_invalid") => {
            "지원하지 않는 형식이거나 손상된 백업이에요. 최신 앱과 원본 파일을 확인해 주세요."
        }
        Some("backup_io_failed") => {
            "백업 파일을 읽거나 저장하지 못했어요. 파일 접근 권한과 남은 공간을 확인해 주세요."
        }
        _ => {
            "복원 작업을 완료하지 못했어요. 백업 파일·암호·로그인 상태를 확인하고 작업 상태를 다시 확인해 주세요."
        }
    }
}

#[tauri::command]
pub(crate) async fn server_backup(
    window: WebviewWindow,
    mut request: Value,
) -> Result<Value, String> {
    crate::caller_is_bundled_ui(&window)?;
    if request.get("action").and_then(Value::as_str) == Some("start") {
        if request.get("destination").is_some() {
            return Err("저장 위치는 파일 선택 창에서 선택해 주세요.".into());
        }
        let selected = rfd::AsyncFileDialog::new()
            .add_filter("AgentsAssemble 서버 백업", &["aaserver"])
            .set_file_name(format!(
                "server-backup-{}.aaserver",
                chrono::Utc::now().format("%Y-%m-%d")
            ))
            .save_file()
            .await;
        let Some(selected) = selected else {
            return Ok(json!({"cancelled":true}));
        };
        request["destination"] = json!(selected.path());
    }
    let app = window.app_handle().clone();
    let ticket = tauri::async_runtime::spawn_blocking(move || {
        app.state::<crate::LocalRuntime>()
            .issue_operator_http_ticket(&app)
    })
    .await
    .map_err(|_| "서버 권한을 확인하지 못했어요.")??;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| "백업 연결을 준비하지 못했어요.")?;
    let mut response = client
        .post(format!("{}/api/server-backup", ticket.http_base_url))
        .bearer_auth(ticket.ticket)
        .json(&request)
        .send()
        .await
        .map_err(|_| "백업 요청 결과를 확인하지 못했어요.")?;
    if !response.status().is_success() {
        let status = response.status();
        eprintln!("server backup control rejected: status={}", status.as_u16());
        return Err(match status.as_u16() {
            403 => "서버 소유 권한을 확인하지 못했어요. 소유 계정의 연결 상태를 확인해 주세요.",
            409 => "진행 중인 백업이나 서버 등록 상태를 확인해 주세요.",
            400 => "백업 요청이 올바르지 않아요. 비밀번호와 저장 위치를 다시 확인해 주세요.",
            _ => "백업 작업을 시작하지 못했어요. 서버 로그를 확인하고 다시 시도해 주세요.",
        }
        .into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "백업 응답을 받지 못했어요.")?
    {
        if bytes.len() + chunk.len() > 8192 {
            return Err("백업 응답이 올바르지 않아요.".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "백업 응답이 올바르지 않아요.".into())
}
