use std::os::unix::fs::PermissionsExt;

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

use super::ProviderAdapter;
use crate::filesystem::{canonical_workspace, executable_identity};
use crate::profile::runtime_profile_key;

#[tokio::test]
async fn codex_runtime_is_initialized_reused_and_stopped_by_exact_owner() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create runtime fixture: {error}"));
    let executable = directory.path().join("codex-fixture");
    std::fs::write(
        &executable,
        b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nwhile :; do sleep 1; done\n",
    )
    .unwrap_or_else(|error| panic!("write runtime fixture: {error}"));
    let mut permissions = std::fs::metadata(&executable)
        .unwrap_or_else(|error| panic!("read runtime fixture mode: {error}"))
        .permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions)
        .unwrap_or_else(|error| panic!("make runtime fixture executable: {error}"));
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("canonicalize runtime fixture: {error}"))
        .to_string_lossy()
        .into_owned();
    let executable_identity = executable_identity(executable.clone())
        .await
        .unwrap_or_else(|error| panic!("identify runtime fixture: {error:?}"));
    let workspace = directory.path().to_string_lossy().into_owned();
    let (workspace, workspace_identity) = canonical_workspace(workspace)
        .await
        .unwrap_or_else(|error| panic!("identify runtime workspace: {error:?}"));
    let session = session(
        executable,
        executable_identity,
        &workspace,
        &workspace_identity,
    );
    let adapter = ProviderAdapter::new();
    let first = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("start Codex fixture: {error}"));
    assert!(!first.runtime_reused);
    assert!(!first.runtime_handle_id.is_empty());
    assert!(!first.runtime_owner_id.is_empty());
    assert!(!first.provider_session_active);
    let second = adapter
        .start(&session)
        .await
        .unwrap_or_else(|error| panic!("reuse Codex fixture: {error}"));
    assert!(second.runtime_reused);
    assert_eq!(second.runtime_handle_id, first.runtime_handle_id);
    assert_eq!(second.runtime_owner_id, first.runtime_owner_id);
    std::fs::write(
        &session.executable,
        b"provider bytes changed while runtime is alive",
    )
    .unwrap_or_else(|error| panic!("change selected executable bytes: {error}"));
    let Err(changed) = adapter.start(&session).await else {
        panic!("changed authority must not be reported as safe while its runtime is alive");
    };
    assert!(changed.effect_uncertain);
    assert_eq!(changed.runtime_handle_id, first.runtime_handle_id);
    assert_eq!(changed.runtime_owner_id, first.runtime_owner_id);
    adapter
        .stop(
            &session.public.room_id,
            &session.public.session_id,
            &first.runtime_handle_id,
            &first.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("stop Codex fixture: {error}"));
    adapter
        .release_confirmed_stop(
            &session.public.room_id,
            &session.public.session_id,
            &first.runtime_handle_id,
            &first.runtime_owner_id,
        )
        .await;
    adapter
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown runtime owner: {error}"));
}

fn session(
    executable: String,
    executable_identity: String,
    workspace: &str,
    workspace_identity: &str,
) -> DurableAgentSession {
    let mut session = serde_json::from_value::<DurableAgentSession>(serde_json::json!({
        "room_id": "general",
        "session_id": "codex-agent",
        "participant_id": "codex-agent",
        "display_name": "Codex",
        "status": "available",
        "runtime_status": "starting",
        "enabled": true,
        "provider_kind": "codex_live_session",
        "runtime_kind": "live_cli",
        "connection_kind": "native_cli_bridge",
        "external_owned": false,
        "process_ownership": "server",
        "model": "gpt-5.6-terra",
        "reasoning_effort": "high",
        "service_tier": "default",
        "variant": "",
        "execution_harness": "builtin",
        "permission_mode": "meeting_read_only",
        "max_output_tokens": 0,
        "catalog_revision": "revision",
        "transport": "stdio_jsonl",
        "last_seen_event_id": "",
        "last_seen_seq": 0,
        "last_provider_sync_event_id": "",
        "last_provider_sync_seq": 0,
        "bootstrap_cutoff_seq": 0,
        "turn_count": 0,
        "created_at": "2026-08-23T00:00:00Z",
        "updated_at": "2026-08-23T00:00:00Z",
        "workspace": workspace,
        "workspace_identity": workspace_identity,
        "runtime_profile_key": "codex-profile",
        "runtime_profile_version": CURRENT_RUNTIME_PROFILE_VERSION
    }))
    .unwrap_or_else(|error| panic!("decode runtime session: {error}"));
    session.executable = executable;
    session.executable_identity = executable_identity;
    session.runtime_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.transport.as_str(),
    ]);
    session
}
