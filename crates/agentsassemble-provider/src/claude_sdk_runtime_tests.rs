use std::os::unix::fs::PermissionsExt;

use super::*;

#[tokio::test]
async fn fatal_protocol_does_not_mask_confirmed_owned_cleanup() {
    let workspace = tempfile::tempdir().unwrap_or_else(|error| panic!("workspace: {error}"));
    let workspace_path = workspace
        .path()
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonical workspace: {error}"));
    let suffix = uuid::Uuid::new_v4().to_string();
    let mut lease = HeldRuntimeLease::prepare("claude-fatal", &suffix)
        .unwrap_or_else(|error| panic!("lease: {error}"));
    let lease_path = lease.path().to_owned();
    lease
        .begin_launch_effect()
        .unwrap_or_else(|error| panic!("launch: {error}"));
    let mut runtime = fatal_runtime(&workspace_path, &lease).await;
    let endpoint = runtime.room_portal.endpoint().to_owned();
    let error = runtime
        .turn("failed-turn", "input")
        .await
        .err()
        .unwrap_or_else(|| panic!("fatal must reject the turn"));
    assert_eq!(error.code, "provider_protocol_invalid");
    assert!(runtime.requires_restart());
    assert!(runtime.turn("retry", "input").await.is_err());
    assert!(
        runtime
            .process_group
            .leader_is_running()
            .await
            .unwrap_or(false)
    );
    runtime
        .stop()
        .await
        .unwrap_or_else(|error| panic!("owned cleanup: {error}"));
    assert!(runtime.requires_restart());
    assert!(
        !crate::unix_process_tree::tagged_runtime_exists(lease.token())
            .unwrap_or_else(|error| panic!("inspect exact process tree: {error}"))
    );
    assert!(
        !crate::runtime_lease::provider_lifetime_is_active(&lease_path, lease.token())
            .unwrap_or_else(|error| panic!("inspect process lifetime: {error}"))
    );
    assert!(
        reqwest::get(endpoint).await.is_err(),
        "portal listener must be closed"
    );
    runtime
        .stop()
        .await
        .unwrap_or_else(|error| panic!("repeat exact cleanup: {error}"));
    lease.release_and_remove();
    assert!(
        !lease_path.exists(),
        "confirmed custody permits lease removal"
    );
}

async fn fatal_runtime(workspace_path: &Path, lease: &HeldRuntimeLease) -> ClaudeSdkRuntime {
    let guardian =
        GuardianLaunch::test_harness().unwrap_or_else(|error| panic!("guardian: {error}"));
    let script = r#"#!/bin/sh
IFS= read -r initialize
printf '%s\n' '{"type":"ready","session_id":"6a1843af-3a9d-44d3-8b0c-41672c83e0dd","reused":false,"model":"claude-opus-5"}'
read -r turn
printf '%s\n' '{"type":"fatal","code":"sdk_query_failed"}'
while read -r command; do :; done
"#;
    let executable = workspace_path.join("claude-host-fixture");
    std::fs::write(&executable, script).unwrap_or_else(|error| panic!("write fixture: {error}"));
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("fixture permissions: {error}"));
    let path = executable.to_string_lossy().into_owned();
    let identity = crate::filesystem::executable_identity(path.clone())
        .await
        .unwrap_or_else(|error| panic!("identify fixture: {error:?}"));
    let shell = bind_executable_with_children(path.clone(), identity.clone())
        .await
        .unwrap_or_else(|error| panic!("bind fixture: {error:?}"));
    let claude_guard = bind_executable(path, identity)
        .await
        .unwrap_or_else(|error| panic!("bind companion fixture: {error:?}"));
    let (process_group, pipes) =
        UnixProcessCustody::start_with_children(lease, &guardian, &shell, &[], &[], workspace_path)
            .await
            .unwrap_or_else(|error| panic!("start fixture: {error:?}"));
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("portal: {error}"));
    let mut session = crate::test_support::durable_session(
        "claude-fatal",
        lease.token(),
        "Claude",
        "claude_live_session",
        "claude-opus-5",
        "stdio",
    );
    session.workspace = workspace_path.to_string_lossy().into_owned();
    let (client, _) = connect_client(pipes.stdin, pipes.stdout, &session, &portal)
        .await
        .unwrap_or_else(|error| panic!("connect fixture: {error:?}"));
    ClaudeSdkRuntime {
        process_group,
        _node_guard: shell,
        _claude_guard: claude_guard,
        _private_claude: None,
        _sdk_bundle: crate::claude_sdk_assets::fixture_bundle(),
        client,
        stderr_task: tokio::spawn(drain_stderr(pipes.stderr)),
        room_portal: portal,
    }
}
