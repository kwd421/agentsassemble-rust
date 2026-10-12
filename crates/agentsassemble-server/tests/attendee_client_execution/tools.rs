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
    store: &agentsassemble_persistence::SqliteStore,
    events: tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
    receiver: &mut mpsc::Receiver<ProviderRoomToolCommand>,
    attachment: (
        &str,
        &str,
        &mut mpsc::Receiver<agentsassemble_provider::ProviderAttachmentReadCommand>,
    ),
) -> TestResult {
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
    );
    let attachment_id = attachment.0.to_owned();
    let large_id = attachment.1.to_owned();
    let store = store.clone();
    let delegate = store.snapshot("general", 0, 200).await?.agent_sessions[0]
        .participant_id
        .clone();
    let mut native = tokio::spawn(async move {
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
        let page = search.content[0]
            .as_text()
            .map_or("", |text| text.text.as_str());
        let results: Vec<_> = page.lines().filter(|line| line.starts_with('#')).collect();
        assert_eq!(results.len(), 1, "search page: {page}");
        assert!(results[0].contains("Reply through the external runtime"));
        let event_id = results[0]
            .split_once(" [event ")
            .and_then(|(_, result)| result.split_once("]: "))
            .map_or_else(
                || panic!("search result must expose an event ID: {page}"),
                |(event_id, _)| event_id,
            );
        call(
            &client,
            "read_message_context",
            json!({"event_id":event_id}),
        )
        .await;
        call(&client, "roll_dice", json!({"notation":"2d6+1"})).await;
        let status = call(&client, "read_room_status", json!({})).await;
        let status: Value =
            serde_json::from_str(status.content[0].as_text().map_or("", |text| &text.text))
                .unwrap_or_else(|error| panic!("room status: {error}"));
        assert!(
            status["agents"]
                .as_array()
                .is_some_and(|agents| !agents.is_empty())
        );
        assert!(status["open_votes"].is_array());
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
        let refused = client
            .call_tool(
                CallToolRequestParams::new("read_attachment").with_arguments(
                    serde_json::from_value(json!({"attachment_id":large_id})).unwrap(),
                ),
            )
            .await
            .unwrap();
        assert_eq!(refused.is_error, Some(true));
        let message = &refused.content[0].as_text().unwrap().text;
        assert!(message.contains("attachment_too_large"));
        assert!(message.contains("10485761") && message.contains("10485760"));
        crate::delegated_pin::verify(&client, &store, &delegate, event_id, None, events)
            .await
            .unwrap();
        let _ = client.cancel().await;
    });
    for index in 0..4 {
        let command = tokio::time::timeout(Duration::from_secs(10), receiver.recv())
            .await?
            .ok_or("native tool request missing")?;
        let call = AttendeeToolCall::new(command).await?;
        let result = call.execute(client, connection).await?;
        if index == 2 {
            assert!(matches!(result, ProviderRoomToolResult::Random(_)));
            // Keep the same call after a lost result; a second HTTP attempt cannot reroll.
            let recovered = call.execute(client, connection).await?;
            assert_eq!(result, recovered);
        }
        call.complete(Ok(result));
    }
    let command = tokio::time::timeout(Duration::from_secs(10), attachment.2.recv())
        .await?
        .ok_or("attachment request missing")?;
    let result = client
        .read_provider_attachment(connection, &command)
        .await?;
    assert_eq!(result.content, b"external attachment proof");
    command.complete(Ok(result));
    let command = tokio::time::timeout(Duration::from_secs(10), attachment.2.recv())
        .await?
        .ok_or("oversized read missing")?;
    let failure = client
        .read_provider_attachment(connection, &command)
        .await
        .expect_err("metadata limit");
    assert_eq!(failure.code, "attachment_too_large");
    assert!(failure.message.contains("10485761") && failure.message.contains("10485760"));
    command.complete(Err(failure.into()));

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            tokio::select! {
                result = &mut native => { result?; return Ok::<_, Box<dyn std::error::Error>>(()); }
                command = receiver.recv() => {
                    let call = AttendeeToolCall::new(command.ok_or("pin relay ended")?).await?;
                    let result = call.execute(client, connection).await.map_err(|error| agentsassemble_provider::ProviderRoomToolError {code:error.code.into(), message:"Attendee pin rejected".into()});
                    call.complete(result);
                }
            }
        }
    }).await??;
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
