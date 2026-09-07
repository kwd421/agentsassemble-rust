use agent_client_protocol::schema::{
    ProtocolVersion,
    v1::{
        AgentCapabilities, ContentBlock, ContentChunk, HttpHeader, InitializeResponse,
        LoadSessionResponse, McpCapabilities, McpServer, McpServerHttp, NewSessionResponse,
        PermissionOption, PermissionOptionKind, PromptResponse, RequestPermissionOutcome,
        RequestPermissionRequest, SessionConfigOption, SessionConfigOptionCategory,
        SessionConfigSelectOption, SessionId, SessionNotification, SessionUpdate,
        SetSessionConfigOptionResponse, StopReason, TextContent, ToolCallUpdate,
        ToolCallUpdateFields,
    },
};
use serde_json::{Value, json};
use std::sync::Mutex;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, DuplexStream},
    sync::oneshot,
    task::JoinHandle,
};

use super::{
    AcpClient, AcpPermissionPolicy, MAX_PROTOCOL_LINE_BYTES, ProtocolState, permission_response,
    record_tool_identity,
};

#[tokio::test]
async fn typed_acp_session_selects_the_exact_model_and_collects_one_turn() {
    let (mut client, fixture_task, _prompt_seen) = fixture(false).await;
    let attached = client
        .attach("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
        .await
        .unwrap_or_else(|error| panic!("attach ACP session: {error}"));
    assert_eq!(attached.session_id, "cursor-session");
    assert!(!attached.reused);

    let turn = client
        .prompt("turn-1", "Hello", false)
        .await
        .unwrap_or_else(|error| panic!("complete ACP prompt: {error}"));
    assert_eq!(turn.turn_id, "turn-1");
    assert_eq!(turn.provider_turn_id, "turn-1");
    assert_eq!(turn.provider_session_id.as_deref(), Some("cursor-session"));
    assert_eq!(
        turn.outcome,
        crate::ProviderTurnOutcome::Message {
            content: "Hello from Cursor".to_owned(),
            target_agent_id: String::new(),
        }
    );
    assert!(!client.requires_restart());
    client.shutdown().await;
    fixture_task
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

#[tokio::test]
async fn tool_only_completion_is_reserved_for_room_publication_validation() {
    for room_observation in [false, true] {
        let (mut client, fixture_task, _) = fixture(false).await;
        client
            .attach("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
            .await
            .unwrap_or_else(|error| panic!("attach ACP session: {error}"));
        let result = client
            .prompt("tool-turn", "Tool only", room_observation)
            .await;
        if room_observation {
            let turn = result.unwrap_or_else(|error| panic!("allow portal validation: {error}"));
            assert_eq!(turn.provider_session_id.as_deref(), Some("cursor-session"));
            assert_eq!(
                turn.outcome,
                crate::ProviderTurnOutcome::Message {
                    content: String::new(),
                    target_agent_id: String::new(),
                }
            );
        } else {
            assert!(result.is_err(), "empty non-room turn must fail");
        }
        client.shutdown().await;
        fixture_task
            .await
            .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
    }
}

#[tokio::test]
async fn cancellation_waits_for_the_exact_acp_cancelled_receipt() {
    let (mut client, fixture, prompt_seen) = fixture(true).await;
    client
        .attach("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
        .await
        .unwrap_or_else(|error| panic!("attach ACP session: {error}"));
    let mut pending = Box::pin(client.prompt("turn-cancel", "Wait", false));
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
    let (mut client, fixture_task, _prompt_seen) = fixture(false).await;
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
    fixture_task
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

#[tokio::test]
async fn process_selected_model_must_match_before_session_creation() {
    let (mut client, fixture_task, _prompt_seen) = fixture(false).await;
    let attached = client
        .attach_process_model("/tmp", "", mcp_server(), "gpt-5.6-sol-high-fast")
        .await
        .unwrap_or_else(|error| panic!("attach process-selected ACP model: {error}"));
    assert_eq!(attached.session_id, "cursor-session");
    client.shutdown().await;
    fixture_task
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));

    let (mut client, fixture_task, _prompt_seen) = fixture(false).await;
    let Err(error) = client
        .attach_process_model("/tmp", "", mcp_server(), "different-model")
        .await
    else {
        panic!("accepted mismatched process-selected model");
    };
    assert_eq!(error.code, "provider_model_unconfirmed");
    assert!(client.requires_restart());
    client.shutdown().await;
    fixture_task
        .await
        .unwrap_or_else(|error| panic!("join ACP fixture: {error}"));
}

#[test]
fn room_tool_permission_requires_exact_active_bound_authority() {
    let state = Mutex::new(ProtocolState {
        permission_policy: AcpPermissionPolicy::RoomTools,
        room_observation_active: true,
        session_id: Some(SessionId::new("session")),
        active_turn_id: Some("turn".to_owned()),
        ..ProtocolState::default()
    });
    let request = permission_request("call", "agentsassemble_room__read_discussion");
    assert_selected(&permission_response(&state, &request), "allow");

    state
        .lock()
        .unwrap_or_else(|_| panic!("lock protocol state"))
        .room_observation_active = false;
    assert_selected(&permission_response(&state, &request), "reject");

    state
        .lock()
        .unwrap_or_else(|_| panic!("lock protocol state"))
        .room_observation_active = true;
    let impostor = permission_request("call", "read_discussion_backup");
    assert_selected(&permission_response(&state, &impostor), "reject");

    let mut locked = state
        .lock()
        .unwrap_or_else(|_| panic!("lock protocol state"));
    record_tool_identity(
        &mut locked.active_tools,
        "call".to_owned(),
        Some(&json!({"tool_name": "read_discussion"})),
    );
    record_tool_identity(
        &mut locked.active_tools,
        "call".to_owned(),
        Some(&json!({"tool_name": "publish_message"})),
    );
    drop(locked);
    assert_selected(&permission_response(&state, &request), "reject");

    state
        .lock()
        .unwrap_or_else(|_| panic!("lock protocol state"))
        .permission_policy = AcpPermissionPolicy::Reject;
    assert_selected(&permission_response(&state, &request), "reject");
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
    let client = AcpClient::connect(client_input, client_output, AcpPermissionPolicy::Reject)
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
                respond(&mut output, id, initialize_response()).await;
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
            "session/prompt"
                if message.pointer("/params/prompt/0/text") == Some(&json!("Tool only")) =>
            {
                respond(&mut output, id, PromptResponse::new(StopReason::EndTurn)).await;
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

fn initialize_response() -> InitializeResponse {
    let meta = serde_json::Map::from_iter([(
        "modelState".to_owned(),
        json!({"currentModelId": "gpt-5.6-sol-high-fast"}),
    )]);
    InitializeResponse::new(ProtocolVersion::V1)
        .agent_capabilities(
            AgentCapabilities::new()
                .load_session(true)
                .mcp_capabilities(McpCapabilities::new().http(true)),
        )
        .meta(meta)
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

fn permission_request(call_id: &str, tool_name: &str) -> RequestPermissionRequest {
    RequestPermissionRequest::new(
        "session",
        ToolCallUpdate::new(
            call_id.to_owned(),
            ToolCallUpdateFields::new().raw_input(json!({"tool_name": tool_name})),
        ),
        vec![
            PermissionOption::new("allow", "Allow once", PermissionOptionKind::AllowOnce),
            PermissionOption::new("reject", "Reject once", PermissionOptionKind::RejectOnce),
        ],
    )
}

fn assert_selected(response: &super::RequestPermissionResponse, option_id: &str) {
    let RequestPermissionOutcome::Selected(selected) = &response.outcome else {
        panic!("permission response did not select an option");
    };
    assert_eq!(selected.option_id.to_string(), option_id);
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
