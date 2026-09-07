use crate::{LocalRuntime, caller_is_bundled_ui, local_runtime::CentralLoginGrant};
use agentsassemble_protocol::CentralLoginAction;
use tauri::{Manager, WebviewWindow};

#[tauri::command]
pub(crate) async fn runtime_central_login(
    window: WebviewWindow,
    app: tauri::AppHandle,
    action: CentralLoginAction,
    state: String,
) -> Result<CentralLoginGrant, String> {
    caller_is_bundled_ui(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<LocalRuntime>()
            .central_login(&app, action, &state)
    })
    .await
    .map_err(|_| "Google login control worker failed".to_owned())?
}

#[tauri::command]
pub(crate) async fn open_central_google_login(
    window: WebviewWindow,
    app: tauri::AppHandle,
    url: String,
) -> Result<(), String> {
    caller_is_bundled_ui(&window)?;
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<LocalRuntime>()
            .open_central_google_login(&app, &url)
    })
    .await
    .map_err(|_| "Google login browser worker failed".to_owned())?
}
