fn main() {
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&["runtime_ticket", "choose_local_workspace"]),
    ))
    .unwrap_or_else(|error| panic!("failed to build AgentsAssemble desktop metadata: {error}"));
}
