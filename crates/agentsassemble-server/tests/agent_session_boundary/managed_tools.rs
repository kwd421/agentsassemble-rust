use super::*;
use rmcp::{
    ServiceExt,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

pub(super) async fn prepare<S>(
    server: &RunningServer,
    socket: &mut RoomSocketPeer<S>,
    snapshot: &Value,
) -> Vec<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_command(socket, "managed-tools-mode", "room.settings.update", &json!({
        "expected_revision":snapshot["room_settings"]["settings_revision"], "tool_mode":"tabletop"
    })).await;
    let _ = receive_command_ack(socket).await;
    let attachment = server
        .state
        .store
        .store_local_message_attachment(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            "managed-proof.txt",
            "text/plain",
            b"managed attachment proof".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("managed attachment: {error}"));
    vec![attachment.id]
}

pub(super) async fn verify(endpoint: &str, token: &str, attachment_id: &str) {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
        ))
        .await
        .unwrap_or_else(|error| panic!("managed native MCP connect: {error}"));
    let read = room_portal_fixture::call_tool(&client, "read_discussion", json!({})).await;
    assert_ne!(read.is_error, Some(true));
    let search = room_portal_fixture::call_tool(
        &client,
        "search_messages",
        json!({"query":"answer the first room message"}),
    )
    .await;
    assert_ne!(search.is_error, Some(true));
    let page: Value =
        serde_json::from_str(search.content[0].as_text().map_or("", |text| &text.text))
            .unwrap_or_else(|error| panic!("managed search page: {error}"));
    assert_eq!(page["results"].as_array().map(Vec::len), Some(1));
    let context = room_portal_fixture::call_tool(
        &client,
        "read_message_context",
        json!({"event_id":page["results"][0]["event_id"]}),
    )
    .await;
    assert_ne!(context.is_error, Some(true));
    let roll =
        room_portal_fixture::call_tool(&client, "roll_dice", json!({"notation":"2d6+1"})).await;
    assert_ne!(roll.is_error, Some(true));
    let attachment = room_portal_fixture::call_tool(
        &client,
        "read_attachment",
        json!({"attachment_id":attachment_id}),
    )
    .await;
    assert_ne!(attachment.is_error, Some(true));
    assert_eq!(
        attachment.content[0]
            .as_text()
            .map(|text| text.text.as_str()),
        Some("managed attachment proof")
    );
    client
        .cancel()
        .await
        .unwrap_or_else(|error| panic!("close managed MCP: {error}"));
}
