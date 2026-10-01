macro_rules! desktop_commands {
    ($($command:ident => $permission:literal),+ $(,)?) => {
        const REGISTERED_COMMAND_NAMES: &[&str] = &[$(stringify!($command)),+];
    };
}

include!("command_registry.rs");

fn main() {
    let central_environment = std::fs::read_to_string("../../frontend/.env.production")
        .unwrap_or_else(|error| panic!("failed to read central environment: {error}"));
    let central_url = central_environment
        .lines()
        .find_map(|line| line.strip_prefix("VITE_AGENTSASSEMBLE_CENTRAL_URL="))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| panic!("production central URL is missing"));
    println!("cargo:rerun-if-changed=../../frontend/.env.production");
    println!("cargo:rustc-env=AGENTSASSEMBLE_CENTRAL_URL={central_url}");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(REGISTERED_COMMAND_NAMES)),
    )
    .unwrap_or_else(|error| panic!("failed to build AgentsAssemble desktop metadata: {error}"));
}
