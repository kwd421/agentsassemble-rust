use agentsassemble_domain::DurableAgentSession;

use super::{
    MAX_PENDING_NOTIFICATION_BYTES, MAX_PENDING_NOTIFICATIONS, command_arguments,
    next_notification_budget,
};
use crate::room_portal::RoomPortal;

#[tokio::test]
async fn command_uses_app_server_and_process_local_profile_settings() {
    let mut session = serde_json::from_value::<DurableAgentSession>(serde_json::json!({
        "room_id": "room",
        "session_id": "agent",
        "participant_id": "agent",
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
        "service_tier": "priority",
        "variant": "",
        "execution_harness": "builtin",
        "permission_mode": "workspace_write",
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
        "workspace": "/tmp/work space",
        "runtime_profile_key": "profile"
    }))
    .unwrap_or_else(|error| panic!("decode session fixture: {error}"));
    session.executable = "/bin/codex".to_owned();
    let room_portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create room portal: {error}"));
    let arguments = command_arguments(&session, &room_portal)
        .unwrap_or_else(|error| panic!("build app-server command: {error}"));
    assert_eq!(arguments.first().map(String::as_str), Some("app-server"));
    assert_eq!(arguments.last().map(String::as_str), Some("--stdio"));
    assert!(
        arguments
            .iter()
            .any(|value| value == "model=\"gpt-5.6-terra\"")
    );
    assert!(
        arguments
            .iter()
            .any(|value| value == "approval_policy=\"on-request\"")
    );
    assert!(
        arguments
            .iter()
            .any(|value| value == "sandbox_mode=\"workspace-write\"")
    );
    assert!(
        arguments
            .iter()
            .any(|value| { value == "projects.\"/tmp/work space\".trust_level=\"trusted\"" })
    );
    assert!(
        arguments
            .iter()
            .any(|value| value.starts_with("mcp_servers.agentsassemble_room.url="))
    );
    assert!(!arguments.iter().any(|value| value == "print"));
}

#[test]
fn pending_notifications_have_an_encoded_byte_budget() {
    assert_eq!(
        next_notification_budget(0, 0, MAX_PENDING_NOTIFICATION_BYTES),
        Ok(MAX_PENDING_NOTIFICATION_BYTES)
    );
    assert!(next_notification_budget(0, MAX_PENDING_NOTIFICATION_BYTES, 1).is_err());
    assert!(next_notification_budget(MAX_PENDING_NOTIFICATIONS, 0, 1).is_err());
}
