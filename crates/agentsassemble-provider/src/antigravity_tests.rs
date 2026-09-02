use agentsassemble_domain::DurableAgentSession;

use super::{command_arguments, require_native_receipt};
use crate::test_support::durable_session;

fn session() -> DurableAgentSession {
    let mut session = durable_session(
        "room-1",
        "agy-1",
        "Antigravity",
        "antigravity_live_session",
        "gemini-3.6-flash",
        "pty",
    );
    session.public.status = "stopped".to_owned();
    session.public.runtime_status = "stopped".to_owned();
    session.public.enabled = false;
    session.public.reasoning_effort = "medium".to_owned();
    session.public.catalog_revision = "catalog".to_owned();
    session.executable = "/usr/bin/agy".to_owned();
    session.executable_identity = "sha256:test".to_owned();
    session.workspace = "/workspace".to_owned();
    session.workspace_identity = "workspace:test".to_owned();
    session
}

#[test]
fn command_is_persistent_and_never_uses_print_mode() {
    let mut session = session();
    assert_eq!(
        command_arguments(&session).unwrap_or_else(|error| panic!("new command: {error}")),
        ["--model", "gemini-3.6-flash-medium", "--sandbox"]
    );
    "conversation-1".clone_into(&mut session.provider_session_id);
    "workspace_write".clone_into(&mut session.public.permission_mode);
    let arguments =
        command_arguments(&session).unwrap_or_else(|error| panic!("resume command: {error}"));
    assert_eq!(
        arguments,
        [
            "--model",
            "gemini-3.6-flash-medium",
            "--mode",
            "accept-edits",
            "--conversation",
            "conversation-1"
        ]
    );
    assert!(arguments.iter().all(|argument| !matches!(
        argument.as_str(),
        "--print" | "-p" | "--prompt" | "--prompt-interactive" | "-i"
    )));

    "../another-conversation".clone_into(&mut session.provider_session_id);
    assert!(command_arguments(&session).is_err());
}

#[test]
fn launch_fails_safely_before_any_native_effect_without_a_receipt_contract() {
    let Err(error) = require_native_receipt() else {
        panic!("Antigravity must not launch without an exact native receipt");
    };

    assert_eq!(error.error.code, "provider_native_receipt_unavailable");
    assert!(!error.effect_uncertain);
}
