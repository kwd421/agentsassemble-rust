macro_rules! desktop_commands {
    ($($command:ident => $permission:literal),+ $(,)?) => {
        const REGISTERED_COMMAND_NAMES: &[&str] = &[$(stringify!($command)),+];
    };
}

include!("command_registry.rs");

fn main() {
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(REGISTERED_COMMAND_NAMES)),
    )
    .unwrap_or_else(|error| panic!("failed to build AgentsAssemble desktop metadata: {error}"));
}
