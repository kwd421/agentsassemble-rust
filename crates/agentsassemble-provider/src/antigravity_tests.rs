use std::{fs, io::Write};

use agentsassemble_domain::DurableAgentSession;
use serde_json::json;

use super::{AntigravityTranscript, command_arguments};

fn session() -> DurableAgentSession {
    serde_json::from_value(json!({
        "room_id": "room-1", "session_id": "agy-1", "participant_id": "agy-1",
        "display_name": "Antigravity", "status": "stopped", "runtime_status": "stopped",
        "enabled": false, "provider_kind": "antigravity_live_session", "runtime_kind": "live_cli",
        "connection_kind": "native_cli_bridge", "external_owned": false,
        "process_ownership": "server", "model": "gemini-3.6-flash",
        "reasoning_effort": "medium", "service_tier": "", "variant": "",
        "execution_harness": "builtin", "permission_mode": "meeting_read_only",
        "max_output_tokens": 0, "catalog_revision": "catalog", "transport": "pty",
        "last_seen_event_id": "", "last_seen_seq": 0, "last_provider_sync_event_id": "",
        "last_provider_sync_seq": 0, "bootstrap_cutoff_seq": 0, "turn_count": 0,
        "created_at": "2026-08-23T00:00:00Z", "updated_at": "2026-08-23T00:00:00Z",
        "executable": "/usr/bin/agy", "executable_identity": "sha256:test",
        "workspace": "/workspace", "workspace_identity": "workspace:test",
        "runtime_profile_key": "profile"
    }))
    .unwrap_or_else(|error| panic!("decode Antigravity session fixture: {error}"))
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
