mod local_runtime;
#[cfg(windows)]
mod private_fs;
mod room_directory_cache;
mod runtime_supervisor;

use agentsassemble_protocol::{HostProductSurface, LocalBootstrapGrant};
use local_runtime::{LocalRuntime, OperatorHttpTicketGrant, TicketGrant};
use serde::{Deserialize, Serialize};
use tauri::{Manager, RunEvent, WebviewWindow};

struct RegisteredCommand {
    name: &'static str,
    permission: &'static str,
}

#[derive(Deserialize)]
struct DesktopCapability {
    permissions: Vec<String>,
}

macro_rules! desktop_commands {
    ($($command:ident => $permission:literal),+ $(,)?) => {
        const REGISTERED_COMMANDS: &[RegisteredCommand] = &[
            $(RegisteredCommand {
                name: stringify!($command),
                permission: $permission,
            },)+
        ];

        macro_rules! registered_invoke_handler {
            () => {
                tauri::generate_handler![$($command),+]
            };
        }
    };
}

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

#[tauri::command]
async fn runtime_bootstrap_status(
    window: WebviewWindow,
    app: tauri::AppHandle,
) -> Result<LocalBootstrapGrant, String> {
    caller_is_bundled_ui(&window)?;
    let runtime_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime_app
            .state::<LocalRuntime>()
            .bootstrap_status(&runtime_app)
    })
    .await
    .map_err(|error| format!("runtime bootstrap status worker failed: {error}"))?
}

#[tauri::command(rename_all = "camelCase")]
async fn runtime_bootstrap_initialize(
    window: WebviewWindow,
    app: tauri::AppHandle,
    request_id: String,
    display_name: String,
) -> Result<LocalBootstrapGrant, String> {
    caller_is_bundled_ui(&window)?;
    let runtime_app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        runtime_app.state::<LocalRuntime>().initialize_bootstrap(
            &runtime_app,
            &request_id,
            &display_name,
        )
    })
    .await
    .map_err(|error| format!("runtime bootstrap initialize worker failed: {error}"))?
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

#[tauri::command]
async fn host_product_surface(
    window: WebviewWindow,
    surface: tauri::State<'_, HostProductSurface>,
) -> Result<HostProductSurface, String> {
    caller_is_bundled_ui(&window)?;
    Ok(surface.inner().clone())
}

include!("../command_registry.rs");

fn registered_host_product_surface() -> HostProductSurface {
    let capability: DesktopCapability =
        serde_json::from_str(include_str!("../capabilities/desktop.json"))
            .unwrap_or_else(|error| panic!("desktop capability registry is invalid: {error}"));
    let commands = REGISTERED_COMMANDS
        .iter()
        .filter(|command| {
            capability
                .permissions
                .iter()
                .any(|permission| permission == command.permission)
        })
        .map(|command| command.name.to_owned())
        .collect();
    HostProductSurface::from_commands(commands)
        .unwrap_or_else(|error| panic!("desktop product-surface registry is invalid: {error}"))
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
        .manage(registered_host_product_surface())
        .invoke_handler(registered_invoke_handler!())
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
    use super::{registered_host_product_surface, workspace_selection};

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

    #[test]
    fn host_surface_is_the_registered_permission_intersection() {
        let surface = registered_host_product_surface();
        assert_eq!(surface.commands.len(), 7);
        assert!(
            surface
                .commands
                .iter()
                .any(|command| command == "host_product_surface")
        );
        assert!(
            !surface
                .commands
                .iter()
                .any(|command| command == "open_central_google_login")
        );
    }
}
