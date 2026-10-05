use std::{process::Stdio, time::Duration};

use agentsassemble_protocol::HostDeviceInfo;
use tauri::{Manager, WebviewWindow};
use tokio::io::AsyncReadExt;

#[tauri::command]
pub(crate) async fn host_device_info(window: WebviewWindow) -> Result<HostDeviceInfo, String> {
    crate::caller_is_bundled_ui(&window)?;
    let database = window
        .app_handle()
        .path()
        .app_data_dir()
        .map_err(|error| format!("cannot resolve installation directory: {error}"))?
        .join("runtime.sqlite3");
    let desktop = std::env::current_exe().map_err(|error| error.to_string())?;
    let executable = crate::local_runtime::sidecar_executable(&desktop)?;
    // A bounded one-shot read: no runtime, TCP listener, credentials or provider state.
    let mut command = tokio::process::Command::new(executable);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let mut child = command
        .arg("--inspect-host-device")
        .arg(database)
        .env_remove("AGENTSASSEMBLE_HOST_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("installation inspection failed: {error}"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or("installation inspection output is missing")?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let mut output = Vec::new();
        stdout.take(4097).read_to_end(&mut output).await?;
        let status = child.wait().await?;
        Ok::<_, std::io::Error>((status, output))
    })
    .await;
    match result {
        Ok(Ok((status, output))) if status.success() && output.len() <= 4096 => {
            serde_json::from_slice(&output)
                .map_err(|_| "installation inspection response is invalid".to_owned())
        }
        _ => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(
                "이 기기의 서버 정보를 확인하지 못했어요. 기존 데이터와 실행 상태를 확인해 주세요."
                    .to_owned(),
            )
        }
    }
}
