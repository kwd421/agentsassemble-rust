mod local_runtime;

use local_runtime::{LocalRuntime, TicketGrant};
use tauri::{Manager, RunEvent, WebviewWindow};

fn caller_is_bundled_ui(window: &WebviewWindow) -> Result<(), String> {
    let url = window
        .url()
        .map_err(|error| format!("cannot inspect desktop caller: {error}"))?;
    let bundled = (url.scheme() == "tauri" && url.host_str() == Some("localhost"))
        || (matches!(url.scheme(), "http" | "https") && url.host_str() == Some("tauri.localhost"));
    if bundled {
        Ok(())
    } else {
        Err("runtime tickets are available only to the bundled desktop UI".to_owned())
    }
}

#[tauri::command(rename_all = "camelCase")]
async fn runtime_ticket(
    window: WebviewWindow,
    app: tauri::AppHandle,
    room_id: String,
) -> Result<TicketGrant, String> {
    caller_is_bundled_ui(&window)?;
    let runtime_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime_app
            .state::<LocalRuntime>()
            .issue_ticket(&runtime_app, &room_id)
    })
    .await
    .map_err(|error| format!("runtime ticket worker failed: {error}"))?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// Runs the desktop shell until its application event loop exits.
///
/// # Panics
///
/// Panics when Tauri cannot initialize the bundled application context.
pub fn run() {
    let app = tauri::Builder::default()
        .manage(LocalRuntime::default())
        .invoke_handler(tauri::generate_handler![runtime_ticket])
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| panic!("AgentsAssemble desktop failed to initialize: {error}"));
    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            handle.state::<LocalRuntime>().stop();
        }
    });
}
