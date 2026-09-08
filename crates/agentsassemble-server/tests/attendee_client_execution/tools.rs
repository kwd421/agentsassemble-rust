use super::{Duration, TestResult};
use agentsassemble_provider::{ProviderRoomToolCommand, ProviderRoomToolResult};
use agentsassemble_server::{AttendeeToolCall, RoomAttendeeClient};
use rmcp::{
    ServiceExt,
    model::CallToolRequestParams,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

pub(super) async fn verify(
    client: &RoomAttendeeClient,
    connection: Uuid,
    endpoint: &str,
    token: &str,
    receiver: &mut mpsc::Receiver<ProviderRoomToolCommand>,
    attachment: (
        &str,
        &mut mpsc::Receiver<agentsassemble_provider::ProviderAttachmentReadCommand>,
    ),
) -> TestResult {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
    );
    let attachment_id = attachment.0.to_owned();
    let native = tokio::spawn(async move {
        let client = ()
            .serve(transport)
            .await
            .unwrap_or_else(|error| panic!("native portal connect: {error}"));
        call(&client, "read_discussion", json!({})).await;
        let search = call(
            &client,
            "search_messages",
            json!({"query":"Reply through the external runtime"}),
        )
        .await;
        let page: Value =
            serde_json::from_str(search.content[0].as_text().map_or("", |text| &text.text))
                .unwrap_or_else(|error| panic!("search page: {error}"));
        assert_eq!(page["results"].as_array().map(Vec::len), Some(1));
        call(
            &client,
            "read_message_context",
            json!({"event_id":page["results"][0]["event_id"]}),
        )
        .await;
        call(&client, "roll_dice", json!({"notation":"2d6+1"})).await;
        let attachment = call(
            &client,
            "read_attachment",
            json!({"attachment_id":attachment_id}),
        )
        .await;
        assert_eq!(
            attachment.content[0]
                .as_text()
                .map(|text| text.text.as_str()),
            Some("external attachment proof")
        );
        let _ = client.cancel().await;
    });
    for index in 0..3 {
        let command = tokio::time::timeout(Duration::from_secs(10), receiver.recv())
            .await?
            .ok_or("native tool request missing")?;
        let call = AttendeeToolCall::new(command)?;
        let result = call.execute(client, connection).await?;
        if index == 2 {
            assert!(matches!(result, ProviderRoomToolResult::Random(_)));
            // Keep the same call after a lost result; a second HTTP attempt cannot reroll.
            let recovered = call.execute(client, connection).await?;
            assert_eq!(result, recovered);
        }
        call.complete(Ok(result));
    }
    let command = tokio::time::timeout(Duration::from_secs(10), attachment.1.recv())
        .await?
        .ok_or("attachment request missing")?;
    let result = client
        .read_provider_attachment(connection, &command)
        .await?;
    assert_eq!(result.content, b"external attachment proof");
    command.complete(Ok(result));
    tokio::time::timeout(Duration::from_secs(10), native).await??;
    Ok(())
}

async fn call(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    name: &'static str,
    arguments: Value,
) -> rmcp::model::CallToolResult {
    let result = client
        .call_tool(
            CallToolRequestParams::new(name).with_arguments(
                serde_json::from_value(arguments)
                    .unwrap_or_else(|error| panic!("tool arguments: {error}")),
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("native tool call: {error}"));
    assert_ne!(result.is_error, Some(true));
    result
}
