use std::{collections::BTreeSet, sync::Arc};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Map, Value, json};

use super::{RoomObservationStart, RoomPortal};
use crate::{ProviderAttachment, ProviderAttachmentReadIngress};

const ATTACHMENT_ID: &str = "ma_11111111111111111111111111111111";
const SECOND_ATTACHMENT_ID: &str = "ma_22222222222222222222222222222222";

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
    let payload: Value = serde_json::from_str(result_text(&result))
        .unwrap_or_else(|error| panic!("decode attachment tool result: {error}"));
    assert_eq!(payload["id"], ATTACHMENT_ID);
    assert_eq!(payload["filename"], "diagram.png");
    assert_eq!(payload["data_base64"], STANDARD.encode([1, 2, 3, 4]));

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

async fn connect(portal: &RoomPortal) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
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
    client: Arc<rmcp::service::RunningService<rmcp::RoleClient, ()>>,
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

fn result_text(result: &rmcp::model::CallToolResult) -> &str {
    result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map_or_else(
            || panic!("attachment tool must return text"),
            |content| content.text.as_str(),
        )
}
