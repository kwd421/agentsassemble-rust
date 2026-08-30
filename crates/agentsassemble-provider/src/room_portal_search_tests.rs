use std::sync::Arc;

use agentsassemble_domain::{
    LobbyMessageContext, LobbyMessageSearchPage, LobbyMessageSearchResult,
};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, json};

use super::{
    ProviderRoomToolCommand, ProviderRoomToolIngress, ProviderRoomToolRequest,
    ProviderRoomToolResult, ProviderTurnOutcome, RoomObservationStart, RoomPortal,
};

type RoomClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

#[tokio::test]
async fn search_tools_share_receipt_budget_and_terminal_ordering() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create search portal: {error}"));
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(4);
    begin_search_observation(&portal, ingress);
    let client = Arc::new(connect(&portal).await);
    establish_search_receipt(client.as_ref(), &mut commands).await;

    let search_client = client.clone();
    let pending_search = tokio::spawn(async move {
        call_tool(
            search_client.as_ref(),
            "search_messages",
            json!({"query": "ALPHA", "cursor": ""}),
        )
        .await
    });
    let mut command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive search command"));
    assert_eq!(
        command.request(),
        &ProviderRoomToolRequest::SearchMessages {
            query: "ALPHA".to_owned(),
            cursor: String::new(),
        }
    );
    command
        .begin_execution()
        .unwrap_or_else(|error| panic!("begin search execution: {error}"));
    let page = LobbyMessageSearchPage {
        results: vec![LobbyMessageSearchResult {
            event_id: "message-1".to_owned(),
            seq: 1,
            created_at: "2026-08-30T00:00:00Z".to_owned(),
            author: "Human".to_owned(),
            content: "ALPHA".to_owned(),
            attachment_filenames: Vec::new(),
        }],
        next_cursor: String::new(),
    };
    command.complete(Ok(ProviderRoomToolResult::SearchMessages(page.clone())));
    let result = pending_search
        .await
        .unwrap_or_else(|error| panic!("join search call: {error}"));
    assert_eq!(tool_json::<LobbyMessageSearchPage>(&result), page);

    let context_client = client.clone();
    let pending_context = tokio::spawn(async move {
        call_tool(
            context_client.as_ref(),
            "read_message_context",
            json!({"event_id": "message-1"}),
        )
        .await
    });
    let mut command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive context command"));
    command
        .begin_execution()
        .unwrap_or_else(|error| panic!("begin context execution: {error}"));
    let context = LobbyMessageContext {
        event_id: "message-1".to_owned(),
        events: Vec::new(),
    };
    command.complete(Ok(ProviderRoomToolResult::MessageContext(context.clone())));
    let result = pending_context
        .await
        .unwrap_or_else(|error| panic!("join context call: {error}"));
    assert_eq!(tool_json::<LobbyMessageContext>(&result), context);

    let published = call_tool(
        client.as_ref(),
        "publish_message",
        json!({"content": "found it", "next_agent_id": ""}),
    )
    .await;
    assert_ne!(published.is_error, Some(true));
    assert_eq!(
        portal
            .finish_observation("turn-search", 9)
            .unwrap_or_else(|error| panic!("finish search observation: {error}")),
        ProviderTurnOutcome::Message {
            content: "found it".to_owned(),
            target_agent_id: String::new(),
        }
    );
    let client =
        Arc::try_unwrap(client).unwrap_or_else(|_| panic!("release search client references"));
    let _ = client.cancel().await;
}

fn begin_search_observation(portal: &RoomPortal, ingress: ProviderRoomToolIngress) {
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-search",
            input_up_to_seq: 9,
            durable_turn_generation: 1,
            execution_id: "00000000-0000-4000-8000-000000000001",
            room_view: "Room: General\n#9 Human: find ALPHA",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tabletop_tools: false,
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin search observation: {error}"));
}

async fn establish_search_receipt(
    client: &RoomClient,
    commands: &mut tokio::sync::mpsc::Receiver<ProviderRoomToolCommand>,
) {
    let early = call_tool(
        client,
        "search_messages",
        json!({"query": "ALPHA", "cursor": ""}),
    )
    .await;
    assert_eq!(early.is_error, Some(true));
    assert!(commands.try_recv().is_err());
    assert_eq!(
        call_tool(
            client,
            "roll_dice",
            json!({"notation": "1d6", "reason": ""}),
        )
        .await
        .is_error,
        Some(true)
    );
    let read = call_tool(client, "read_discussion", json!({})).await;
    assert_ne!(read.is_error, Some(true));
}

fn tool_json<T: serde::de::DeserializeOwned>(result: &rmcp::model::CallToolResult) -> T {
    let text = result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map_or_else(
            || panic!("room search tool returned no JSON"),
            |content| content.text.as_str(),
        );
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("decode room search tool JSON: {error}"))
}

async fn call_tool(
    client: &RoomClient,
    name: &'static str,
    arguments: serde_json::Value,
) -> rmcp::model::CallToolResult {
    let arguments = serde_json::from_value::<Map<String, serde_json::Value>>(arguments)
        .unwrap_or_else(|error| panic!("decode tool arguments: {error}"));
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}

async fn connect(portal: &RoomPortal) -> RoomClient {
    ().serve(StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
            .auth_header(portal.bearer_token()),
    ))
    .await
    .unwrap_or_else(|error| panic!("connect search portal: {error}"))
}
