#![cfg(unix)]

use std::{os::unix::fs::PermissionsExt, process::Stdio, time::Duration};

use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

#[tokio::test]
#[allow(clippy::too_many_lines)] // One stdio session proves initialize, discovery, tool calls, files, and clean exit.
async fn internal_stdio_mcp_reads_and_stages_one_room_publication() {
    let root = tempfile::tempdir().unwrap_or_else(|error| panic!("create MCP root: {error}"));
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .unwrap_or_else(|error| panic!("secure MCP root: {error}"));
    std::fs::write(
        root.path().join("view.txt"),
        b"Room: General\n#7 Host: hello",
    )
    .unwrap_or_else(|error| panic!("write MCP view: {error}"));
    std::fs::write(
        root.path().join("turn.json"),
        serde_json::to_vec(&json!({
            "turn_id": "turn-mcp-1",
            "input_up_to_seq": 7,
            "allowed_agent_ids": ["agent-2"],
        }))
        .unwrap_or_else(|error| panic!("encode MCP turn: {error}")),
    )
    .unwrap_or_else(|error| panic!("write MCP turn: {error}"));
    for file in [root.path().join("view.txt"), root.path().join("turn.json")] {
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o600))
            .unwrap_or_else(|error| panic!("secure MCP authority file: {error}"));
    }

    let mut child = Command::new(env!("CARGO_BIN_EXE_agentsassemble-server"))
        .args([
            "--agentsassemble-room-portal-mcp",
            "--root",
            root.path()
                .to_str()
                .unwrap_or_else(|| panic!("MCP root is not UTF-8")),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap_or_else(|error| panic!("spawn internal MCP server: {error}"));
    let mut stdin = child
        .stdin
        .take()
        .unwrap_or_else(|| panic!("MCP server stdin is unavailable"));
    let stdout = child
        .stdout
        .take()
        .unwrap_or_else(|| panic!("MCP server stdout is unavailable"));
    let mut stdout = BufReader::new(stdout);

    let initialized = request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "boundary-test", "version": "1"},
            },
        }),
    )
    .await;
    assert_eq!(initialized["id"], 1);
    notify(
        &mut stdin,
        json!({"jsonrpc": "2.0", "method": "notifications/initialized"}),
    )
    .await;
    let tools = request(
        &mut stdin,
        &mut stdout,
        json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}}),
    )
    .await;
    let mut names = tools["result"]["tools"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tool| tool["name"].as_str())
        .collect::<Vec<_>>();
    names.sort_unstable();
    assert_eq!(
        names,
        ["decline_to_speak", "publish_message", "read_discussion"]
    );
    let read = request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": {"name": "read_discussion", "arguments": {}},
        }),
    )
    .await;
    assert!(response_text(&read).contains("Host: hello"));
    let publish = request(
        &mut stdin,
        &mut stdout,
        json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": {
                "name": "publish_message",
                "arguments": {"content": "room reply", "next_agent_id": "agent-2"},
            },
        }),
    )
    .await;
    assert!(response_text(&publish).contains("Published"));
    let receipt: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("receipt.json"))
            .unwrap_or_else(|error| panic!("read MCP receipt: {error}")),
    )
    .unwrap_or_else(|error| panic!("decode MCP receipt: {error}"));
    assert_eq!(
        receipt,
        json!({"turn_id": "turn-mcp-1", "observed_through_seq": 7})
    );
    let outcome: Value = serde_json::from_slice(
        &std::fs::read(root.path().join("outcome.json"))
            .unwrap_or_else(|error| panic!("read MCP outcome: {error}")),
    )
    .unwrap_or_else(|error| panic!("decode MCP outcome: {error}"));
    assert_eq!(
        outcome,
        json!({
            "kind": "message",
            "turn_id": "turn-mcp-1",
            "content": "room reply",
            "target_agent_id": "agent-2",
        })
    );

    drop(stdin);
    wait_for_exit(&mut child).await;
}

async fn request(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    message: Value,
) -> Value {
    notify(stdin, message).await;
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), stdout.read_line(&mut line))
        .await
        .unwrap_or_else(|_| panic!("MCP response timed out"))
        .unwrap_or_else(|error| panic!("read MCP response: {error}"));
    serde_json::from_str(&line).unwrap_or_else(|error| panic!("decode MCP response: {error}"))
}

async fn notify(stdin: &mut ChildStdin, message: Value) {
    stdin
        .write_all(message.to_string().as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write MCP message: {error}"));
    stdin
        .write_all(b"\n")
        .await
        .unwrap_or_else(|error| panic!("write MCP delimiter: {error}"));
    stdin
        .flush()
        .await
        .unwrap_or_else(|error| panic!("flush MCP message: {error}"));
}

fn response_text(response: &Value) -> &str {
    response["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
}

async fn wait_for_exit(child: &mut Child) {
    let status = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .unwrap_or_else(|_| panic!("MCP server did not exit after stdin closed"))
        .unwrap_or_else(|error| panic!("wait for MCP server: {error}"));
    assert!(status.success());
}
