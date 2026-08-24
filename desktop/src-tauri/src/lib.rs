mod local_runtime;
#[cfg(windows)]
mod private_fs;
mod room_directory_cache;
mod runtime_supervisor;

use local_runtime::{LocalRuntime, OperatorHttpTicketGrant, TicketGrant};
use serde::Serialize;
use tauri::{Manager, RunEvent, WebviewWindow};

#[derive(Serialize)]
struct WorkspaceSelection {
    selected: bool,
    path: String,
}

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

#[tauri::command]
async fn runtime_operator_ticket(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<OperatorHttpTicketGrant, String> {
    caller_is_bundled_ui(&window)?;
    let runtime_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime_app
            .state::<LocalRuntime>()
            .issue_operator_http_ticket(&runtime_app)
    })
    .await
    .map_err(|error| format!("runtime operator ticket worker failed: {error}"))?
}

#[tauri::command]
async fn cache_selected_room_directory(
    window: WebviewWindow,
    app: tauri::AppHandle,
    rooms: String,
) -> Result<(), String> {
    caller_is_bundled_ui(&window)?;
    tauri::async_runtime::spawn_blocking(move || room_directory_cache::store(&app, &rooms))
        .await
        .map_err(|error| format!("room directory cache worker failed: {error}"))?
}

fn workspace_selection(path: Option<std::path::PathBuf>) -> Result<WorkspaceSelection, String> {
    let Some(path) = path else {
        return Ok(WorkspaceSelection {
            selected: false,
            path: String::new(),
        });
    };
    let path = path
        .canonicalize()
        .map_err(|error| format!("workspace_picker_invalid_selection: {error}"))?;
    if !path.is_dir() {
        return Err("workspace_picker_invalid_selection: selection is not a directory".to_owned());
    }
    let path = path
        .to_str()
        .ok_or_else(|| "workspace_picker_invalid_selection: path is not UTF-8".to_owned())?
        .to_owned();
    Ok(WorkspaceSelection {
        selected: true,
        path,
    })
}

#[tauri::command]
async fn choose_local_workspace(window: WebviewWindow) -> Result<WorkspaceSelection, String> {
    caller_is_bundled_ui(&window)?;
    tauri::async_runtime::spawn_blocking(|| {
        workspace_selection(rfd::FileDialog::new().pick_folder())
    })
    .await
    .map_err(|error| format!("workspace_picker_failed: {error}"))?
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
        .invoke_handler(tauri::generate_handler![
            runtime_ticket,
            runtime_operator_ticket,
            cache_selected_room_directory,
            choose_local_workspace
        ])
        .build(tauri::generate_context!())
        .unwrap_or_else(|error| panic!("AgentsAssemble desktop failed to initialize: {error}"));
    app.run(|handle, event| {
        if matches!(event, RunEvent::Exit) {
            handle.state::<LocalRuntime>().stop();
        }
    });
}

#[must_use]
pub fn run_runtime_supervisor_if_requested() -> Option<i32> {
    runtime_supervisor::run_if_requested()
}

#[cfg(test)]
mod tests {
    use super::workspace_selection;

    #[test]
    fn workspace_selection_is_canonical_and_cancel_is_empty() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create workspace fixture: {error}"));
        let selected = workspace_selection(Some(directory.path().to_path_buf()))
            .unwrap_or_else(|error| panic!("select workspace fixture: {error}"));
        assert!(selected.selected);
        let canonical = directory
            .path()
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize workspace fixture: {error}"));
        assert_eq!(selected.path, canonical.to_string_lossy());

        let cancelled = workspace_selection(None)
            .unwrap_or_else(|error| panic!("cancel workspace fixture: {error}"));
        assert!(!cancelled.selected);
        assert!(cancelled.path.is_empty());
    }
}
