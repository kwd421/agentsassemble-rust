use std::{collections::BTreeSet, sync::Arc};

use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

use super::{RoomObservationStart, RoomPortal};
use crate::room_attachment::attachment_from_tool_result;
use crate::{ProviderAttachment, ProviderAttachmentReadCommand, ProviderAttachmentReadIngress};

const ATTACHMENT_ID: &str = "ma_11111111111111111111111111111111";
const SECOND_ATTACHMENT_ID: &str = "ma_22222222222222222222222222222222";
type RoomClient = rmcp::service::RunningService<rmcp::RoleClient, ()>;

#[tokio::test]
async fn exact_turn_mcp_read_returns_one_bounded_attachment() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create attachment portal: {error}"));
    let (ingress, mut commands) = ProviderAttachmentReadIngress::channel(1);
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-attachment",
            input_up_to_seq: 7,
            durable_turn_generation: 3,
            execution_id: "00000000-0000-4000-8000-000000000003",
            room_view: &format!(
                "Room: General\nAttachment `{ATTACHMENT_ID}`\nAttachment `{SECOND_ATTACHMENT_ID}`"
            ),
            attachment_ids: &[ATTACHMENT_ID.to_owned(), SECOND_ATTACHMENT_ID.to_owned()],
            attachment_ingress: Some(ingress),
            allowed_agent_ids: &[],
            tool_ingress: None,
        })
        .unwrap_or_else(|error| panic!("begin attachment observation: {error}"));
    let client = Arc::new(connect(&portal).await);
    let request = tokio::spawn(call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": ATTACHMENT_ID}),
    ));
    let command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive exact attachment command"));
    assert_eq!(command.session_id(), "agent-1");
    assert_eq!(command.turn_id(), "turn-attachment");
    assert_eq!(command.input_up_to_seq(), 7);
    assert_eq!(command.turn_generation(), 3);
    assert_eq!(command.attachment_id(), ATTACHMENT_ID);
    command.complete(Ok(ProviderAttachment {
        id: ATTACHMENT_ID.to_owned(),
        filename: "diagram.png".to_owned(),
        content_type: "image/png".to_owned(),
        size: 4,
        is_image: true,
        content: vec![1, 2, 3, 4],
    }));
    let result = request
        .await
        .unwrap_or_else(|error| panic!("join attachment tool: {error}"));
    assert_ne!(result.is_error, Some(true));
    let attachment = attachment_from_tool_result(&result)
        .unwrap_or_else(|error| panic!("decode attachment tool result: {error}"));
    assert_eq!(attachment.id, ATTACHMENT_ID);
    assert_eq!(attachment.filename, "diagram.png");
    assert_eq!(attachment.content, [1, 2, 3, 4]);
    let retry = tokio::spawn(call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": ATTACHMENT_ID}),
    ));
    let command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive bounded attachment retry"));
    command.complete(Ok(ProviderAttachment {
        id: ATTACHMENT_ID.to_owned(),
        filename: "diagram.png".to_owned(),
        content_type: "image/png".to_owned(),
        size: 4,
        is_image: true,
        content: vec![1, 2, 3, 4],
    }));
    assert_ne!(
        retry
            .await
            .unwrap_or_else(|error| panic!("join bounded attachment retry: {error}"))
            .is_error,
        Some(true)
    );
    let exhausted = call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": ATTACHMENT_ID}),
    )
    .await;
    assert_eq!(exhausted.is_error, Some(true));
    assert!(commands.try_recv().is_err());

    assert_response_validation_and_generic_resource(&client, &mut commands).await;
    let missing = call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": "ma_33333333333333333333333333333333"}),
    )
    .await;
    assert_eq!(missing.is_error, Some(true));
    assert!(commands.try_recv().is_err());
    let client = Arc::try_unwrap(client)
        .unwrap_or_else(|_| panic!("attachment client references must be released"));
    let _ = client.cancel().await;
}

#[tokio::test]
async fn pending_attachment_read_blocks_terminal_action_and_finish() {
    let portal = RoomPortal::create()
        .await
        .unwrap_or_else(|error| panic!("create attachment portal: {error}"));
    let (ingress, mut commands) = ProviderAttachmentReadIngress::channel(1);
    portal
        .begin_observation(RoomObservationStart {
            session_id: "agent-1",
            turn_id: "turn-pending",
            input_up_to_seq: 8,
            durable_turn_generation: 4,
            execution_id: "00000000-0000-4000-8000-000000000004",
            room_view: &format!("Room: General\nAttachment `{ATTACHMENT_ID}`"),
            attachment_ids: &[ATTACHMENT_ID.to_owned()],
            attachment_ingress: Some(ingress),
            allowed_agent_ids: &[],
            tool_ingress: None,
        })
        .unwrap_or_else(|error| panic!("begin attachment observation: {error}"));
    let reader = Arc::new(connect(&portal).await);
    let terminal = Arc::new(connect(&portal).await);
    let _ = call_tool(terminal.clone(), "read_discussion", json!({})).await;
    let pending = tokio::spawn(call_tool(
        reader.clone(),
        "read_attachment",
        json!({"attachment_id": ATTACHMENT_ID}),
    ));
    let command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive pending attachment command"));
    let early = call_tool(
        terminal.clone(),
        "publish_message",
        json!({"content": "reply", "next_agent_id": ""}),
    )
    .await;
    assert_eq!(early.is_error, Some(true));
    assert!(portal.finish_observation("turn-pending", 8).is_err());
    command.complete(Ok(ProviderAttachment {
        id: ATTACHMENT_ID.to_owned(),
        filename: "notes.txt".to_owned(),
        content_type: "text/plain".to_owned(),
        size: 5,
        is_image: false,
        content: b"notes".to_vec(),
    }));
    assert_ne!(
        pending
            .await
            .unwrap_or_else(|error| panic!("join pending attachment read: {error}"))
            .is_error,
        Some(true)
    );
    let published = call_tool(
        terminal.clone(),
        "publish_message",
        json!({"content": "reply", "next_agent_id": ""}),
    )
    .await;
    assert_ne!(published.is_error, Some(true));
    assert!(portal.finish_observation("turn-pending", 8).is_ok());
    let reader = Arc::try_unwrap(reader).unwrap_or_else(|_| panic!("reader references released"));
    let terminal =
        Arc::try_unwrap(terminal).unwrap_or_else(|_| panic!("terminal references released"));
    let _ = reader.cancel().await;
    let _ = terminal.cancel().await;
}

async fn assert_response_validation_and_generic_resource(
    client: &Arc<RoomClient>,
    commands: &mut tokio::sync::mpsc::Receiver<ProviderAttachmentReadCommand>,
) {
    let mismatched = tokio::spawn(call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": SECOND_ATTACHMENT_ID}),
    ));
    let command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive second exact attachment command"));
    command.complete(Ok(ProviderAttachment {
        id: ATTACHMENT_ID.to_owned(),
        filename: "wrong.bin".to_owned(),
        content_type: "application/octet-stream".to_owned(),
        size: 1,
        is_image: false,
        content: vec![1],
    }));
    let mismatched = mismatched
        .await
        .unwrap_or_else(|error| panic!("join mismatched attachment tool: {error}"));
    assert_eq!(mismatched.is_error, Some(true));

    let generic = tokio::spawn(call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": SECOND_ATTACHMENT_ID}),
    ));
    let command = commands
        .recv()
        .await
        .unwrap_or_else(|| panic!("receive retried attachment command"));
    command.complete(Ok(ProviderAttachment {
        id: SECOND_ATTACHMENT_ID.to_owned(),
        filename: "notes.txt".to_owned(),
        content_type: "text/plain".to_owned(),
        size: 5,
        is_image: false,
        content: b"notes".to_vec(),
    }));
    let generic = generic
        .await
        .unwrap_or_else(|error| panic!("join generic attachment tool: {error}"));
    let attachment = attachment_from_tool_result(&generic)
        .unwrap_or_else(|error| panic!("decode generic attachment resource: {error}"));
    assert_eq!(attachment.id, SECOND_ATTACHMENT_ID);
    assert_eq!(attachment.content, b"notes");
    let duplicate = call_tool(
        client.clone(),
        "read_attachment",
        json!({"attachment_id": SECOND_ATTACHMENT_ID}),
    )
    .await;
    assert_eq!(duplicate.is_error, Some(true));
    assert!(commands.try_recv().is_err());
}

async fn connect(portal: &RoomPortal) -> RoomClient {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(portal.endpoint())
                .auth_header(portal.bearer_token()),
        ))
        .await
        .unwrap_or_else(|error| panic!("connect attachment portal: {error}"));
    let names = client
        .list_all_tools()
        .await
        .unwrap_or_else(|error| panic!("list attachment tools: {error}"))
        .into_iter()
        .map(|tool| tool.name.to_string())
        .collect::<BTreeSet<_>>();
    assert!(names.contains("read_attachment"));
    client
}

async fn call_tool(
    client: Arc<RoomClient>,
    name: &'static str,
    arguments: Value,
) -> rmcp::model::CallToolResult {
    let arguments = serde_json::from_value::<Map<String, Value>>(arguments)
        .unwrap_or_else(|error| panic!("decode tool arguments: {error}"));
    client
        .call_tool(CallToolRequestParams::new(name).with_arguments(arguments))
        .await
        .unwrap_or_else(|error| panic!("call {name}: {error}"))
}
