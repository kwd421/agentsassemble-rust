use std::{fs, io::Write};

use agentsassemble_domain::DurableAgentSession;
use serde_json::json;

use super::{
    ActiveTurn, AntigravityDriver, AntigravityRoomPermissionPolicy, AntigravityTranscript,
    command_arguments,
};
use crate::{
    antigravity_transport::AntigravityTerminal,
    room_portal::RoomPortal,
    runtime::{DriverError, DriverFuture, ProviderDriver, ProviderTurnRequest},
    test_support::durable_session,
};
use tokio::time::Instant;
use uuid::Uuid;

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
fn command_is_persistent_and_resume_is_exact() {
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

#[tokio::test]
async fn first_prompt_write_installs_exact_restart_custody() {
    let (_home, mut driver) = failing_write_driver().await;
    let request = turn_request();

    let result = driver.submit_turn_prompt(&request, "bounded prompt").await;

    let Err(error) = result else {
        panic!("failing first prompt write must return an error");
    };
    assert_eq!(error.code, "provider_transport_failed");
    assert_eq!(
        driver.active_turn.as_ref().map(|turn| &turn.request),
        Some(&request)
    );
    assert!(driver.requires_restart());
}

#[tokio::test]
async fn failed_control_c_write_cannot_leave_a_reusable_runtime() {
    let (_home, mut driver) = failing_write_driver().await;
    let request = turn_request();
    driver.active_turn = Some(ActiveTurn {
        request: request.clone(),
        provider_turn_id: "provider-turn-interrupt".to_owned(),
        last_progress: Instant::now(),
    });

    let result = driver.interrupt_turn_exact(&request).await;

    let Err(error) = result else {
        panic!("failing control-C write must return an error");
    };
    assert_eq!(error.code, "provider_transport_failed");
    assert!(driver.poisoned);
    assert!(driver.requires_restart());
}

#[test]
fn transcript_binds_exact_turn_and_ignores_terminal_output() {
    let home = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create Antigravity home fixture: {error}"));
    let workspace = home.path().join("workspace");
    fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create workspace fixture: {error}"));
    let existing = transcript_path(home.path(), "existing");
    write_rows(&existing, &[user_row("old"), final_row("old answer")]);
    let mut source = AntigravityTranscript::new(home.path().to_path_buf(), workspace.clone());
    source
        .prepare_start(None)
        .unwrap_or_else(|error| panic!("prepare source: {error}"));
    source
        .begin_turn("the assigned input")
        .unwrap_or_else(|error| panic!("begin source turn: {error}"));
    assert!(
        source
            .poll()
            .unwrap_or_else(|error| panic!("poll old source: {error}"))
            .is_none()
    );

    let active = transcript_path(home.path(), "conversation-1");
    write_rows(
        &active,
        &[
            user_row("different input"),
            final_row("wrong answer"),
            user_row("the assigned input"),
            json!({"source":"MODEL","type":"PLANNER_RESPONSE","status":"RUNNING","content":"partial"}),
            json!({"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","tool_calls":[{"name":"run_command"}],"content":"tool"}),
            final_row("authoritative answer"),
        ],
    );
    let snapshot = source
        .poll()
        .unwrap_or_else(|error| panic!("poll active source: {error}"))
        .unwrap_or_else(|| panic!("completed transcript missing"));
    assert_eq!(snapshot.content, "authoritative answer");
    assert_eq!(snapshot.provider_session_id, "conversation-1");
}

#[test]
fn resumed_transcript_reads_only_new_rows_from_the_exact_conversation() {
    let home = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create resumed Antigravity home: {error}"));
    let workspace = home.path().join("workspace");
    fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create resumed workspace: {error}"));
    let path = transcript_path(home.path(), "conversation-1");
    write_rows(&path, &[user_row("old input"), final_row("old answer")]);
    let mut source = AntigravityTranscript::new(home.path().to_path_buf(), workspace);
    source
        .prepare_start(Some("conversation-1"))
        .unwrap_or_else(|error| panic!("bind resumed source: {error}"));
    source
        .begin_turn("new input")
        .unwrap_or_else(|error| panic!("begin resumed turn: {error}"));
    assert!(source.poll().unwrap_or(None).is_none());
    let mut transcript = fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap_or_else(|error| panic!("open resumed transcript: {error}"));
    writeln!(transcript, "{}", user_row("new input"))
        .unwrap_or_else(|error| panic!("append resumed input: {error}"));
    writeln!(transcript, "{}", final_row("new answer"))
        .unwrap_or_else(|error| panic!("append resumed final: {error}"));
    let snapshot = source
        .poll()
        .unwrap_or_else(|error| panic!("poll resumed source: {error}"))
        .unwrap_or_else(|| panic!("resumed final missing"));
    assert_eq!(snapshot.content, "new answer");
    assert_eq!(snapshot.provider_session_id, "conversation-1");
}

#[test]
fn new_transcript_binding_rejects_more_than_one_matching_session() {
    let home = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create ambiguous Antigravity home: {error}"));
    let workspace = home.path().join("workspace");
    fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create workspace fixture: {error}"));
    let mut source = AntigravityTranscript::new(home.path().to_path_buf(), workspace);
    source
        .prepare_start(None)
        .unwrap_or_else(|error| panic!("prepare ambiguous source: {error}"));
    source
        .begin_turn("input with launch nonce")
        .unwrap_or_else(|error| panic!("begin ambiguous turn: {error}"));
    for id in ["conversation-1", "conversation-2"] {
        write_rows(
            &transcript_path(home.path(), id),
            &[
                user_row("input with launch nonce"),
                final_row("ambiguous answer"),
            ],
        );
    }
    assert!(source.poll().is_err());
}

#[test]
fn transcript_tail_is_bounded_before_json_allocation() {
    let home = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create bounded Antigravity home: {error}"));
    let workspace = home.path().join("workspace");
    fs::create_dir(&workspace).unwrap_or_else(|error| panic!("create workspace fixture: {error}"));
    let mut source = AntigravityTranscript::new(home.path().to_path_buf(), workspace);
    source
        .prepare_start(None)
        .unwrap_or_else(|error| panic!("prepare bounded source: {error}"));
    source
        .begin_turn("bounded input")
        .unwrap_or_else(|error| panic!("begin bounded turn: {error}"));
    let path = transcript_path(home.path(), "conversation-large");
    fs::create_dir_all(
        path.parent()
            .unwrap_or_else(|| panic!("transcript parent missing")),
    )
    .unwrap_or_else(|error| panic!("create transcript fixture: {error}"));
    fs::write(&path, vec![b'x'; 2 * 1024 * 1024 + 1])
        .unwrap_or_else(|error| panic!("write oversized transcript: {error}"));
    assert!(source.poll().is_err());
}

fn transcript_path(home: &std::path::Path, id: &str) -> std::path::PathBuf {
    home.join(".gemini/antigravity-cli/brain")
        .join(id)
        .join(".system_generated/logs/transcript.jsonl")
}

fn write_rows(path: &std::path::Path, rows: &[serde_json::Value]) {
    fs::create_dir_all(
        path.parent()
            .unwrap_or_else(|| panic!("transcript parent missing")),
    )
    .unwrap_or_else(|error| panic!("create transcript fixture: {error}"));
    let body = rows
        .iter()
        .map(serde_json::Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    fs::write(path, body).unwrap_or_else(|error| panic!("write transcript fixture: {error}"));
}

fn user_row(content: &str) -> serde_json::Value {
    json!({"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","content":content})
}

fn final_row(content: &str) -> serde_json::Value {
    json!({"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","content":content})
}

async fn failing_write_driver() -> (tempfile::TempDir, AntigravityDriver) {
    let home = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create Antigravity driver fixture: {error}"));
    let workspace = home.path().join("workspace");
    fs::create_dir(&workspace)
        .unwrap_or_else(|error| panic!("create Antigravity driver workspace: {error}"));
    let room_portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create Antigravity room portal: {error}"));
    let driver = AntigravityDriver {
        terminal: Box::new(FailingWriteTerminal),
        transcript: AntigravityTranscript::new(home.path().to_path_buf(), workspace),
        room_portal,
        terminal_helper: None,
        hook: None,
        attached_session_id: Some("conversation-1".to_owned()),
        attached_reused: false,
        startup_drained: true,
        terminal_query_tail: Vec::new(),
        permission_policy: AntigravityRoomPermissionPolicy::new(),
        transcript_nonce: Uuid::new_v4(),
        active_turn: None,
        completed_turn: None,
        terminal_tail: Vec::new(),
        poisoned: false,
    };
    (home, driver)
}

fn turn_request() -> ProviderTurnRequest {
    ProviderTurnRequest {
        turn_id: "turn-1".to_owned(),
        turn_generation: 1,
        execution_id: "execution-1".to_owned(),
        input: "test input".to_owned(),
        room_observation: None,
    }
}

struct FailingWriteTerminal;

impl AntigravityTerminal for FailingWriteTerminal {
    fn read(&mut self) -> DriverFuture<'_, Result<Vec<u8>, DriverError>> {
        Box::pin(async { Ok(Vec::new()) })
    }

    fn write<'a>(&'a mut self, _data: &'a [u8]) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async {
            Err(DriverError::new(
                "provider_transport_failed",
                "simulated terminal write failure",
            ))
        })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async { Ok(true) })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async { Ok(()) })
    }

    fn request_stop(&mut self) {}
}
