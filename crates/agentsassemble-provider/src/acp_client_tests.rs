use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{
        AgentCapabilities, ContentBlock, ContentChunk, HttpHeader, InitializeResponse,
        LoadSessionResponse, McpCapabilities, McpServer, McpServerHttp, NewSessionResponse,
        PromptResponse, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigSelectOption, SessionNotification, SessionUpdate,
        SetSessionConfigOptionResponse, StopReason, TextContent,
    },
};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream},
    sync::oneshot,
    task::JoinHandle,
};

use super::{AcpClient, MAX_PROTOCOL_LINE_BYTES};

#[tokio::test]
async fn typed_acp_session_selects_the_exact_model_and_collects_one_turn() {
    let (mut client, fixture, _prompt_seen) = fixture(false).await;
    let attached = client
        .attach("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
        .await
        .unwrap_or_else(|error| panic!("attach ACP session: {error}"));
    assert_eq!(attached.session_id, "cursor-session");
    assert!(!attached.reused);

    let turn = client
        .prompt("turn-1", "Hello")
        .await
        .unwrap_or_else(|error| panic!("complete ACP prompt: {error}"));
    assert_eq!(turn.session_id, "cursor-session");
    assert_eq!(turn.stop_reason, StopReason::EndTurn);
    assert_eq!(turn.output, "Hello from Cursor");
    assert!(!client.requires_restart());
    client.shutdown().await;
    fixture
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

#[tokio::test]
async fn cancellation_waits_for_the_exact_acp_cancelled_receipt() {
    let (mut client, fixture, prompt_seen) = fixture(true).await;
    client
        .attach("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
        .await
        .unwrap_or_else(|error| panic!("attach ACP session: {error}"));
    let mut pending = Box::pin(client.prompt("turn-cancel", "Wait"));
    tokio::select! {
        biased;
        _result = &mut pending => panic!("fixture completed before cancellation"),
        seen = prompt_seen => seen.unwrap_or_else(|_| panic!("observe fixture prompt")),
    }
    drop(pending);
    client
        .cancel("turn-cancel")
        .await
        .unwrap_or_else(|error| panic!("confirm ACP cancellation: {error}"));
    assert!(!client.requires_restart());
    client.shutdown().await;
    fixture
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

#[tokio::test]
async fn durable_session_load_accepts_the_exact_uncategorized_model_option() {
    let (mut client, fixture, _prompt_seen) = fixture(false).await;
    let attached = client
        .attach(
            "/tmp",
            "cursor-session",
            mcp_server(),
            "gpt-5.6-sol-high-fast",
        )
        .await
        .unwrap_or_else(|error| panic!("load ACP session: {error}"));
    assert_eq!(attached.session_id, "cursor-session");
    assert!(attached.reused);
    assert!(!client.requires_restart());
    client.shutdown().await;
    fixture
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

async fn fixture(cancel_prompt: bool) -> (AcpClient, JoinHandle<()>, oneshot::Receiver<()>) {
    let (client_input, fixture_input) = tokio::io::duplex(MAX_PROTOCOL_LINE_BYTES);
    let (fixture_output, client_output) = tokio::io::duplex(MAX_PROTOCOL_LINE_BYTES);
    let (prompt_seen_sender, prompt_seen) = oneshot::channel();
    let task = tokio::spawn(run_fixture(
        fixture_input,
        fixture_output,
        cancel_prompt,
        prompt_seen_sender,
    ));
    let client = AcpClient::connect(client_input, client_output)
        .await
        .unwrap_or_else(|error| panic!("connect ACP fixture: {:?}", error.error));
    (client, task, prompt_seen)
}

async fn run_fixture(
    input: DuplexStream,
    mut output: DuplexStream,
    cancel_prompt: bool,
    prompt_seen: oneshot::Sender<()>,
) {
    let mut lines = BufReader::new(input).lines();
    let mut pending_prompt = None;
    let mut prompt_seen = Some(prompt_seen);
    while let Ok(Some(line)) = lines.next_line().await {
        let message: Value =
            serde_json::from_str(&line).unwrap_or_else(|error| panic!("fixture JSON: {error}"));
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let id = message.get("id").cloned();
        match method {
            "initialize" => {
                respond(
                    &mut output,
                    id,
                    InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(
                        AgentCapabilities::new()
                            .load_session(true)
                            .mcp_capabilities(McpCapabilities::new().http(true)),
                    ),
                )
                .await;
            }
            "session/new" => {
                assert_eq!(
                    message.pointer("/params/mcpServers/0/type"),
                    Some(&json!("http"))
                );
                assert!(
                    message
                        .pointer("/params/mcpServers/0/headers/0/value")
                        .and_then(Value::as_str)
                        .is_some_and(|value| value.starts_with("Bearer "))
                );
                respond(
                    &mut output,
                    id,
                    NewSessionResponse::new("cursor-session")
                        .config_options(vec![model_option("auto")]),
                )
                .await;
            }
            "session/load" => {
                assert_eq!(
                    message.pointer("/params/sessionId"),
                    Some(&json!("cursor-session"))
                );
                respond(
                    &mut output,
                    id,
                    LoadSessionResponse::new().config_options(vec![uncategorized_model_option()]),
                )
                .await;
            }
            "session/set_config_option" => {
                assert_eq!(
                    message.pointer("/params/value"),
                    Some(&json!("gpt-5.6-sol-high-fast"))
                );
                respond(
                    &mut output,
                    id,
                    SetSessionConfigOptionResponse::new(vec![model_option(
                        "gpt-5.6-sol-high-fast",
                    )]),
                )
                .await;
            }
            "session/prompt" if cancel_prompt => {
                pending_prompt = id;
                if let Some(prompt_seen) = prompt_seen.take() {
                    let _ = prompt_seen.send(());
                }
            }
            "session/prompt" => {
                notify(
                    &mut output,
                    SessionNotification::new(
                        "cursor-session",
                        SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
                            TextContent::new("Hello from Cursor"),
                        ))),
                    ),
                )
                .await;
                respond(&mut output, id, PromptResponse::new(StopReason::EndTurn)).await;
            }
            "session/cancel" => {
                respond(
                    &mut output,
                    pending_prompt.take(),
                    PromptResponse::new(StopReason::Cancelled),
                )
                .await;
            }
            _ => panic!("unexpected ACP fixture method: {method}"),
        }
    }
}

fn model_option(current: &str) -> SessionConfigOption {
    SessionConfigOption::select(
        "model",
        "Model",
        current.to_owned(),
        vec![
            SessionConfigSelectOption::new("auto", "Auto"),
            SessionConfigSelectOption::new("gpt-5.6-sol-high-fast", "Sol High Fast"),
        ],
    )
    .category(SessionConfigOptionCategory::Model)
}

fn uncategorized_model_option() -> SessionConfigOption {
    SessionConfigOption::select(
        "model",
        "Model",
        "gpt-5.6-sol-high-fast",
        vec![SessionConfigSelectOption::new(
            "gpt-5.6-sol-high-fast",
            "Sol High Fast",
        )],
    )
}

fn mcp_server() -> McpServer {
    McpServer::Http(
        McpServerHttp::new("agentsassemble_room", "http://127.0.0.1:1/mcp")
            .headers(vec![HttpHeader::new("Authorization", "Bearer test")]),
    )
}

async fn respond(output: &mut DuplexStream, id: Option<Value>, result: impl serde::Serialize) {
    let Some(id) = id else {
        panic!("fixture response requires request id");
    };
    write(
        output,
        json!({"jsonrpc": "2.0", "id": id, "result": result}),
    )
    .await;
}

async fn notify(output: &mut DuplexStream, notification: SessionNotification) {
    write(
        output,
        json!({
            "jsonrpc": "2.0",
            "method": "session/update",
            "params": notification,
        }),
    )
    .await;
}

async fn write(output: &mut DuplexStream, message: Value) {
    let mut encoded = serde_json::to_vec(&message)
        .unwrap_or_else(|error| panic!("encode fixture message: {error}"));
    encoded.push(b'\n');
    output
        .write_all(&encoded)
        .await
        .unwrap_or_else(|error| panic!("write fixture message: {error}"));
    output
        .flush()
        .await
        .unwrap_or_else(|error| panic!("flush fixture message: {error}"));
}
