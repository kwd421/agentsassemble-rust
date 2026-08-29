use std::{collections::BTreeSet, sync::Arc};

use agentsassemble_domain::RoomRandomResult;
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, json};

use super::{ProviderRoomToolIngress, ProviderTurnOutcome, RoomObservationStart, RoomPortal};

type RoomClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;
const EXECUTION_ID: &str = "00000000-0000-4000-8000-000000000001";

#[tokio::test]
async fn reservation_first_orders_random_tool_before_terminal_action() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create tabletop portal: {error}"));
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(4);
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-tabletop",
            input_up_to_seq: 9,
            durable_turn_generation: 1,
            execution_id: EXECUTION_ID,
            room_view: "Room: General\n#9 Human: roll",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin tabletop observation: {error}"));
    let client = Arc::new(connect(&portal).await);
    let control_client = connect(&portal).await;
    let names = client
        .list_all_tools()
        .await
        .unwrap_or_else(|error| panic!("list tabletop tools: {error}"))
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "choose_random".to_owned(),
            "decline_to_speak".to_owned(),
            "publish_message".to_owned(),
            "read_attachment".to_owned(),
            "read_discussion".to_owned(),
            "roll_dice".to_owned(),
        ])
    );
    let read = call_tool(client.as_ref(), "read_discussion", json!({})).await;
    assert_ne!(read.is_error, Some(true));

    let tool_client = client.clone();
    let pending_tool = tokio::spawn(async move {
        call_tool(
            tool_client.as_ref(),
            "roll_dice",
            json!({"notation": "2d6+1", "reason": "initiative"}),
        )
        .await
    });
    let mut command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("room actor must receive the reserved tool command"));
    assert_eq!(command.turn_generation(), 1);
    assert_eq!(command.execution_id(), EXECUTION_ID);
    let blocked_terminal = call_tool(
        &control_client,
        "publish_message",
        json!({"content": "too early"}),
    )
    .await;
    assert_eq!(blocked_terminal.is_error, Some(true));
    command
        .begin_commit()
        .unwrap_or_else(|error| panic!("begin room actor commit: {error}"));
    command.complete(Ok(RoomRandomResult::RollDice {
        notation: "2d6+1".to_owned(),
        rolls: vec![2, 5],
        modifier: 1,
        total: 8,
    }));
    let tool_result = pending_tool
        .await
        .unwrap_or_else(|error| panic!("join tabletop tool call: {error}"));
    assert_ne!(tool_result.is_error, Some(true));

    let published = call_tool(
        &control_client,
        "publish_message",
        json!({"content": "after the roll"}),
    )
    .await;
    assert_ne!(published.is_error, Some(true));
    assert_eq!(
        portal
            .finish_observation("turn-tabletop", 9)
            .unwrap_or_else(|error| panic!("finish tabletop observation: {error}")),
        ProviderTurnOutcome::Message {
            content: "after the roll".to_owned(),
            target_agent_id: String::new(),
        }
    );

    let client = Arc::try_unwrap(client)
        .unwrap_or_else(|_| panic!("tabletop client references must be released"));
    let _ = client.cancel().await;
    let _ = control_client.cancel().await;
}

#[tokio::test]
async fn terminal_first_rejects_late_random_tool() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create terminal-first portal: {error}"));
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-terminal-first",
            input_up_to_seq: 10,
            durable_turn_generation: 1,
            execution_id: EXECUTION_ID,
            room_view: "Room: General\n#10 Human: decide",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin terminal-first observation: {error}"));
    let client = connect(&portal).await;
    let _ = call_tool(&client, "read_discussion", json!({})).await;
    let declined = call_tool(
        &client,
        "decline_to_speak",
        json!({"reason_code": "duplicate"}),
    )
    .await;
    assert_ne!(declined.is_error, Some(true));
    let late_tool = call_tool(
        &client,
        "choose_random",
        json!({"options": ["north", "south"]}),
    )
    .await;
    assert_eq!(late_tool.is_error, Some(true));
    assert!(commands.try_recv().is_err());
    let _ = client.cancel().await;
}

#[tokio::test]
async fn closing_observation_retains_a_committing_tool_until_resolution() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create closing portal: {error}"));
    let (ingress, mut commands) = ProviderRoomToolIngress::channel(1);
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-closing",
            input_up_to_seq: 11,
            durable_turn_generation: 1,
            execution_id: EXECUTION_ID,
            room_view: "Room: General\n#11 Human: roll",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tool_ingress: Some(ingress.clone()),
        })
        .unwrap_or_else(|error| panic!("begin closing observation: {error}"));
    let client = Arc::new(connect(&portal).await);
    let _ = call_tool(client.as_ref(), "read_discussion", json!({})).await;
    let tool_client = client.clone();
    let pending_tool = tokio::spawn(async move {
        call_tool(
            tool_client.as_ref(),
            "choose_random",
            json!({"options": ["north", "south"]}),
        )
        .await
    });
    let mut command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive committing room tool"));
    command
        .begin_commit()
        .unwrap_or_else(|error| panic!("begin committing room tool: {error}"));
    portal
        .end_observation()
        .unwrap_or_else(|error| panic!("close observation: {error}"));
    assert!(
        portal
            .begin_observation(RoomObservationStart {
                session_id: "agent-1",
                turn_id: "turn-too-early",
                input_up_to_seq: 12,
                durable_turn_generation: 2,
                execution_id: "00000000-0000-4000-8000-000000000002",
                room_view: "Room: General\n#12 Human: wait",
                attachment_ids: &[],
                attachment_ingress: None,
                allowed_agent_ids: &[],
                tool_ingress: Some(ingress.clone()),
            })
            .is_err()
    );
    command.complete(Ok(RoomRandomResult::ChooseRandom {
        choice: "north".to_owned(),
        index: 0,
        option_count: 2,
        options: vec!["north".to_owned(), "south".to_owned()],
    }));
    let result = pending_tool
        .await
        .unwrap_or_else(|error| panic!("join committing room tool: {error}"));
    assert_ne!(result.is_error, Some(true));
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-after-close",
            input_up_to_seq: 12,
            durable_turn_generation: 2,
            execution_id: "00000000-0000-4000-8000-000000000002",
            room_view: "Room: General\n#12 Human: continue",
            attachment_ids: &[],
            attachment_ingress: None,
            allowed_agent_ids: &[],
            tool_ingress: Some(ingress),
        })
        .unwrap_or_else(|error| panic!("begin after committing tool resolution: {error}"));
    let client = Arc::try_unwrap(client)
        .unwrap_or_else(|_| panic!("closing client references must be released"));
    let _ = client.cancel().await;
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
    .unwrap_or_else(|error| panic!("connect tabletop portal: {error}"))
}
