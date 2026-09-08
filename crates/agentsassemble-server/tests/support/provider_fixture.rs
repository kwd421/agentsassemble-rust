use agentsassemble_domain::{
    ProviderAvailability, ProviderCatalog, ProviderControl, ProviderControlOption,
};
use std::{collections::BTreeMap, fs::File, path::Path};

use agentsassemble_domain::{
    codex_bundle_identity, codex_code_mode_host_name, stable_content_identity,
};
use same_file::Handle;

pub fn write_codex_bundle(root: &Path, fixture: &[u8]) -> (String, String) {
    let executable = root.join(if cfg!(windows) {
        "provider-fixture.exe"
    } else {
        "provider-fixture"
    });
    let companion = root.join(codex_code_mode_host_name());
    write_executable(&executable, fixture);
    #[cfg(unix)]
    write_executable(
        &companion,
        b"#!/bin/sh\nprintf '%s\\n' 'ws://127.0.0.1:43123'\nexec /usr/bin/tail -f /dev/null\n",
    );
    #[cfg(not(unix))]
    write_executable(&companion, b"codex companion fixture");
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("resolve test executable: {error}"));
    let main_identity = executable_identity(&executable);
    let companion_identity = executable_identity(&companion);
    let bundle_identity = codex_bundle_identity(&main_identity, &companion_identity);
    (executable.to_string_lossy().into_owned(), bundle_identity)
}

fn write_executable(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write test executable: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(path)
            .unwrap_or_else(|error| panic!("read test executable mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(path, permissions)
            .unwrap_or_else(|error| panic!("set test executable mode: {error}"));
    }
}

fn executable_identity(path: &Path) -> String {
    let mut file = File::open(path).unwrap_or_else(|error| panic!("open test executable: {error}"));
    let handle = Handle::from_file(
        file.try_clone()
            .unwrap_or_else(|error| panic!("clone test executable: {error}")),
    )
    .unwrap_or_else(|error| panic!("identify test executable: {error}"));
    stable_content_identity(&handle, &mut file)
        .unwrap_or_else(|error| panic!("hash test executable: {error}"))
}

#[cfg(unix)]
pub const CODEX_FIXTURE: &[u8] = b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\nIFS= read -r forever\n";

pub fn agent_catalog(root: &Path, fixture_override: Option<&[u8]>) -> ProviderCatalog {
    #[cfg(unix)]
    let fixture = CODEX_FIXTURE;
    #[cfg(not(unix))]
    let fixture: &[u8] = b"provider fixture";
    let fixture = fixture_override.unwrap_or(fixture);
    let (executable, executable_identity) = write_codex_bundle(root, fixture);
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "catalog-boundary-1".to_owned(),
        discovered_at: "2026-08-22T00:00:00Z".to_owned(),
        providers: vec![ProviderAvailability {
            id: "codex".to_owned(),
            display_name: "Codex".to_owned(),
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            catalog_group: "harness".to_owned(),
            workspace_required: true,
            connection_kind: "native_cli_bridge".to_owned(),
            executable,
            executable_identity,
            default_model: "gpt-5.6-terra".to_owned(),
            interactive: true,
            turn_interrupt: agentsassemble_domain::ProviderTurnInterrupt::Unsupported,
            startable: true,
            available: true,
            discovery_status: "ready".to_owned(),
            catalog_source: "discovered".to_owned(),
            discovery_error_code: String::new(),
            discovery_error: String::new(),
            credential_available: false,
            custom_endpoint: false,
            custom_model: false,
            login_supported: false,
            controls: vec![
                ProviderControl {
                    key: "model".to_owned(),
                    label: "Model".to_owned(),
                    kind: "combobox".to_owned(),
                    options: vec![ProviderControlOption {
                        value: "gpt-5.6-terra".to_owned(),
                        label: "Terra".to_owned(),
                        metadata: BTreeMap::default(),
                    }],
                    default_value: "gpt-5.6-terra".to_owned(),
                },
                ProviderControl {
                    key: "permission_mode".to_owned(),
                    label: "Permission".to_owned(),
                    kind: "select".to_owned(),
                    options: vec![ProviderControlOption {
                        value: "meeting_read_only".to_owned(),
                        label: "Read only".to_owned(),
                        metadata: BTreeMap::default(),
                    }],
                    default_value: "meeting_read_only".to_owned(),
                },
            ],
        }],
    }
}
