use agentsassemble_domain::DurableAgentSession;

use super::{
    MAX_PENDING_NOTIFICATION_BYTES, MAX_PENDING_NOTIFICATIONS, command_arguments,
    is_room_portal_approval, next_notification_budget,
};
use crate::{
    room_portal::{ROOM_PORTAL_TOKEN_ENV_PREFIX, RoomPortal},
    test_support::durable_session,
};

#[tokio::test]
async fn command_uses_app_server_and_process_local_profile_settings() {
    let mut session = codex_session_fixture();
    session.executable = "/bin/codex".to_owned();
    let room_portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create room portal: {error}"));
    let arguments = command_arguments(
        &session,
        &room_portal,
        &["company.tools".to_owned(), "node_repl".to_owned()],
    )
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
    assert!(arguments.iter().any(|value| {
        value == "projects={ \"/tmp/work space\" = { trust_level = \"untrusted\" } }"
    }));
    assert!(
        arguments
            .iter()
            .any(|value| value.starts_with("mcp_servers.agentsassemble_room.url="))
    );
    let bearer_environment_name = room_portal.bearer_environment_name();
    assert!(bearer_environment_name.starts_with(ROOM_PORTAL_TOKEN_ENV_PREFIX));
    let second_room_portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create second room portal: {error}"));
    assert_ne!(
        bearer_environment_name,
        second_room_portal.bearer_environment_name()
    );
    assert!(arguments.iter().any(|value| {
        value
            == &format!(
                "mcp_servers.agentsassemble_room.bearer_token_env_var=\"{bearer_environment_name}\""
            )
    }));
    assert!(arguments.iter().any(|value| {
        value == "mcp_servers.agentsassemble_room.default_tools_approval_mode=\"approve\""
    }));
    assert!(
        arguments
            .iter()
            .any(|value| { value == "shell_environment_policy.ignore_default_excludes=false" })
    );
    assert!(
        arguments
            .iter()
            .any(|value| value == "features.plugins=false")
    );
    assert!(arguments.iter().any(|value| value == "features.apps=false"));
    assert!(arguments.iter().any(|value| {
        value
            == "mcp_servers={ \"company.tools\" = { enabled = false }, \"node_repl\" = { enabled = false } }"
    }));
    assert!(
        arguments
            .iter()
            .any(|value| value == "features.shell_snapshot=false")
    );
    let provider_environment = room_portal.provider_environment();
    let token = provider_environment
        .iter()
        .find_map(|(name, value)| (name == bearer_environment_name).then_some(value.as_str()))
        .unwrap_or_else(|| panic!("room portal bearer environment is missing"));
    assert!(!token.is_empty());
    assert!(!arguments.iter().any(|value| value.contains(token)));
    assert!(!arguments.iter().any(|value| value == "print"));
}

fn codex_session_fixture() -> DurableAgentSession {
    let mut session = durable_session(
        "room",
        "agent",
        "Codex",
        "codex_live_session",
        "gpt-5.6-terra",
        "stdio_jsonl",
    );
    session.public.service_tier = "priority".to_owned();
    session.public.permission_mode = "workspace_write".to_owned();
    session.workspace = "/tmp/work space".to_owned();
    session
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

#[test]
fn only_the_exact_room_portal_mcp_approval_is_accepted() {
    let approval = serde_json::json!({
        "method": "mcpServer/elicitation/request",
        "params": {
            "serverName": "agentsassemble_room",
            "_meta": {"codex_approval_kind": "mcp_tool_call"},
        },
    });
    assert!(is_room_portal_approval(&approval));
    for changed in [
        serde_json::json!({
            "method": "mcpServer/elicitation/request",
            "params": {
                "serverName": "other",
                "_meta": {"codex_approval_kind": "mcp_tool_call"},
            },
        }),
        serde_json::json!({
            "method": "mcpServer/elicitation/request",
            "params": {
                "serverName": "agentsassemble_room",
                "_meta": {"codex_approval_kind": "other"},
            },
        }),
        serde_json::json!({
            "method": "command_execution/request_approval",
            "params": {
                "serverName": "agentsassemble_room",
                "_meta": {"codex_approval_kind": "mcp_tool_call"},
            },
        }),
    ] {
        assert!(!is_room_portal_approval(&changed));
    }
}
