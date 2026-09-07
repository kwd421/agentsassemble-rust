use std::path::Path;

use super::{ProviderAdapter, fixture_session};

#[tokio::test]
async fn codex_home_selection_binds_config_and_child() {
    const CHILD_ROOT: &str = "AGENTSASSEMBLE_TEST_CODEX_HOME_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        verify_child_home(Path::new(&root)).await;
        return;
    }
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create home fixture: {error}"));
    let default_home = root.path().join("home");
    let custom_home = root.path().join("custom");
    for (home, server) in [
        (default_home.join(".codex"), "default_server"),
        (custom_home.clone(), "custom_server"),
    ] {
        std::fs::create_dir_all(&home)
            .unwrap_or_else(|error| panic!("create config home: {error}"));
        std::fs::write(
            home.join("config.toml"),
            format!("[mcp_servers.{server}]\ncommand = 'fixture-only'\n"),
        )
        .unwrap_or_else(|error| panic!("write config fixture: {error}"));
    }
    for custom in [true, false] {
        let mut child = tokio::process::Command::new(
            std::env::current_exe().unwrap_or_else(|error| panic!("locate test binary: {error}")),
        );
        child
            .args([
                "--exact",
                "runtime::tests::codex_home_tests::codex_home_selection_binds_config_and_child",
                "--nocapture",
            ])
            .env(CHILD_ROOT, root.path())
            .env("HOME", &default_home)
            .env("USERPROFILE", &default_home);
        if custom {
            child.env("CODEX_HOME", &custom_home);
        } else {
            child.env_remove("CODEX_HOME");
        }
        let output = child
            .output()
            .await
            .unwrap_or_else(|error| panic!("run isolated home fixture: {error}"));
        assert!(
            output.status.success(),
            "home fixture failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed"),
            "isolated home verification did not execute exactly one test"
        );
    }
}

async fn verify_child_home(root: &Path) {
    let custom = std::env::var_os("CODEX_HOME").is_some();
    let expected_home = if custom {
        root.join("custom")
    } else {
        root.join("home/.codex")
    };
    let expected_server = if custom {
        "custom_server"
    } else {
        "default_server"
    };
    let other_server = if custom {
        "default_server"
    } else {
        "custom_server"
    };
    let workspace = root.join(if custom {
        "custom-workspace"
    } else {
        "default-workspace"
    });
    std::fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create workspace: {error}"));
    let mut session = fixture_session(&workspace, concat!(
        "#!/bin/sh\n",
        "printf '%s\\n' \"$CODEX_HOME\" > observed-home\n",
        "printf '%s\\n' \"$@\" > observed-arguments\n",
        "IFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\n",
        "IFS= read -r initialized\nIFS= read -r thread\n",
        "printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\n",
        "IFS= read -r forever\n",
    )).await;
    session.public.room_id = format!("codex-home-{}", uuid::Uuid::new_v4());
    let adapter = ProviderAdapter::new();
    let started = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start exact-home runtime: {error}"));
    let observed_home = std::fs::read_to_string(workspace.join("observed-home"));
    let observed_arguments = std::fs::read_to_string(workspace.join("observed-arguments"));
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("stop exact-home runtime: {error}"));
    assert_eq!(
        observed_home
            .unwrap_or_else(|error| panic!("read child home: {error}"))
            .trim(),
        expected_home.to_string_lossy()
    );
    let arguments =
        observed_arguments.unwrap_or_else(|error| panic!("read child arguments: {error}"));
    assert!(arguments.contains(&format!("\"{expected_server}\" = {{ enabled = false }}")));
    assert!(!arguments.contains(other_server));
    let home =
        crate::codex::config::home().unwrap_or_else(|error| panic!("resolve probe home: {error}"));
    let output = crate::process::probe(
        "/bin/sh",
        &["-c", "printf '%s' \"$CODEX_HOME\""],
        &tokio_util::sync::CancellationToken::new(),
        &[("CODEX_HOME".to_owned(), home)],
    )
    .await
    .unwrap_or_else(|error| panic!("probe exact home: {error:?}"));
    assert_eq!(output, expected_home.to_string_lossy());
}
