use std::{fmt::Write, path::Path, time::Duration};

use serde_json::{Value, json};

use super::{
    AGENT_BOUNDARY_LOCK, AuthenticatedTestSocket, agent_catalog_with_fixture, bootstrap, connect,
    receive_command_ack, receive_json, receive_json_with_timeout, send_command, send_create, start,
    subscribe,
};

#[tokio::test]
async fn busy_turn_interrupt_is_exact_and_runtime_retaining() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create interrupt root: {error}"));
    let transcript = directory.path().join("interrupt-requests.jsonl");
    let turn_seen = directory.path().join("interrupt-turn-seen");
    let fixture = interrupt_fixture(&transcript, &turn_seen);
    let store = agentsassemble_persistence::SqliteStore::open(&format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("open interrupt store: {error}"));
    bootstrap(&store).await;
    let server = start(
        store,
        agent_catalog_with_fixture(directory.path(), fixture.as_bytes()),
    )
    .await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let session_id = create_started_agent(&mut socket, directory.path()).await;
    let control_payload = json!({"agent_id": session_id});
    send_command(
        &mut socket,
        "interrupt-source-message",
        "message.send",
        &json!({"content": "@Terra remain active until interrupted"}),
    )
    .await;
    let _message = receive_command_ack(&mut socket).await;
    super::room_portal_fixture::wait_for_turn(&turn_seen, "seen").await;

    send_command(
        &mut socket,
        "interrupt-busy-turn",
        "agent.interrupt",
        &control_payload,
    )
    .await;
    let interrupted = receive_command_ack(&mut socket).await;
    assert_eq!(interrupted["action"], "agent.interrupt");
    assert_eq!(interrupted["result"]["interrupt_requested"], true);
    assert!(interrupted["deduplicated"].is_null());

    let (terminal_events, terminal_state) = receive_terminal_state(&mut socket).await;
    assert!(terminal_events.iter().any(|event| event == "error"));
    assert!(terminal_events.iter().any(|event| event == "turn_finished"));
    assert_eq!(terminal_state["status"], "attached");
    assert_eq!(terminal_state["runtime_status"], "idle");
    assert_eq!(terminal_state["provider_session_active"], true);
    assert_eq!(terminal_state["active_turn_id"], "");
    assert_eq!(terminal_state["last_error_code"], "interrupted");

    assert_one_start_and_interrupt(&transcript);

    let mut replay_socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut replay_socket).await;
    let replay_snapshot = receive_json(&mut replay_socket).await;
    assert_eq!(
        replay_snapshot["agent_sessions"][0]["runtime_status"],
        "idle"
    );
    send_command(
        &mut replay_socket,
        "interrupt-busy-turn",
        "agent.interrupt",
        &control_payload,
    )
    .await;
    let replay = receive_command_ack(&mut replay_socket).await;
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], interrupted["result"]);
    replay_socket.close().await;
    socket.close().await;
    server.stop().await;
}

async fn create_started_agent<S>(socket: &mut AuthenticatedTestSocket<S>, root: &Path) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_create(
        socket,
        "create-interrupt-agent",
        &json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-boundary-1",
            "display_name": "Terra",
            "workspace": root,
            "model": "gpt-5.6-terra",
            "permission_mode": "meeting_read_only",
            "start_now": false,
        }),
    )
    .await;
    let created = receive_command_ack(socket).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created interrupt session has no id"))
        .to_owned();
    send_command(
        socket,
        "start-interrupt-agent",
        "agent.start",
        &json!({"agent_id": session_id}),
    )
    .await;
    let _started = receive_command_ack(socket).await;
    session_id
}

async fn receive_terminal_state<S>(socket: &mut AuthenticatedTestSocket<S>) -> (Vec<String>, Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut terminal_events = Vec::new();
    for _ in 0..8 {
        let frame = receive_json_with_timeout(socket, Duration::from_secs(5)).await;
        for event in frame["events"].as_array().into_iter().flatten() {
            terminal_events.push(
                event["type"]
                    .as_str()
                    .unwrap_or_else(|| panic!("room event has no type"))
                    .to_owned(),
            );
            if event["type"] == "agent_session_state"
                && event["agent_session"]["runtime_status"] == "idle"
            {
                return (terminal_events, event["agent_session"].clone());
            }
        }
    }
    panic!("interrupt terminal state was not published");
}

fn assert_one_start_and_interrupt(transcript: &Path) {
    let requests = std::fs::read_to_string(transcript)
        .unwrap_or_else(|error| panic!("read interrupt transcript: {error}"))
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|error| panic!("decode interrupt request: {error}"))
        })
        .collect::<Vec<_>>();
    for method in ["turn/start", "turn/interrupt"] {
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == method)
                .count(),
            1,
            "provider method {method} must occur exactly once",
        );
    }
}

fn interrupt_fixture(transcript: &Path, turn_seen: &Path) -> String {
    format!(
        "#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' \"$initialize\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nprintf '%s\\n' \"$initialized\" >> {log}\nIFS= read -r thread\nprintf '%s\\n' \"$thread\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r turn\nprintf '%s\\n' \"$turn\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":3,\"result\":{{\"turn\":{{\"id\":\"provider-turn-1\"}}}}}}'\nprintf seen > {seen}\nIFS= read -r interrupt\nprintf '%s\\n' \"$interrupt\" >> {log}\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":4,\"result\":{{}}}}'\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"method\":\"turn/completed\",\"params\":{{\"threadId\":\"thread-1\",\"turnId\":\"provider-turn-1\"}}}}'\nIFS= read -r forever\n",
        log = shell_quote(transcript),
        seen = shell_quote(turn_seen),
    )
}

fn shell_quote(path: &Path) -> String {
    let mut quoted = String::from("'");
    for character in path.to_string_lossy().chars() {
        if character == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted
                .write_char(character)
                .unwrap_or_else(|error| panic!("quote fixture path: {error}"));
        }
    }
    quoted.push('\'');
    quoted
}
