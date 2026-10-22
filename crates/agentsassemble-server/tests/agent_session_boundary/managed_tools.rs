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
    workspace: &Path,
) -> Vec<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use std::io::Write;
    send_command(socket, "managed-tools-mode", "room.settings.update", &json!({
        "expected_revision":snapshot["room_settings"]["settings_revision"], "tool_mode":"tabletop", "attachment_file_limit_bytes":50*1024*1024
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
    let mut source = std::fs::File::create(workspace.join("managed-large.bin")).unwrap();
    for _ in 0..480 {
        source.write_all(&vec![71; 65536]).unwrap();
    }
    let large = server
        .state
        .store
        .store_local_message_attachment(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            "large-context.bin",
            "application/octet-stream",
            vec![71; 30 * 1024 * 1024],
        )
        .await
        .unwrap();
    vec![attachment.id, large.id]
}

pub(super) async fn verify(
    endpoint: &str,
    token: &str,
    attachment_id: &str,
    large_attachment_id: &str,
    store: &SqliteStore,
    delegate: &str,
    events: tokio::sync::broadcast::Receiver<agentsassemble_domain::RoomEvent>,
    owner: (&str, &agentsassemble_server::AppState),
) -> String {
    use std::io::Read;
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
    let page = search.content[0]
        .as_text()
        .map_or("", |text| text.text.as_str());
    let results: Vec<_> = page.lines().filter(|line| line.starts_with('#')).collect();
    assert_eq!(results.len(), 1, "managed search page: {page}");
    assert!(results[0].contains("answer the first room message"));
    let event_id = results[0]
        .split_once(" [event ")
        .and_then(|(_, result)| result.split_once("]: "))
        .map_or_else(
            || panic!("managed search result must expose an event ID: {page}"),
            |(event_id, _)| event_id,
        );
    let context = room_portal_fixture::call_tool(
        &client,
        "read_message_context",
        json!({"event_id":event_id}),
    )
    .await;
    assert_ne!(context.is_error, Some(true));
    let context_text = context.content[0]
        .as_text()
        .map_or("", |text| text.text.as_str());
    assert!(context_text.contains(&format!("<- the result [event {event_id}]")));
    assert!(context_text.contains("answer the first room message"));
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
    let large = room_portal_fixture::call_tool(
        &client,
        "read_attachment",
        json!({"attachment_id":large_attachment_id}),
    )
    .await;
    assert_ne!(large.is_error, Some(true), "{large:?}");
    let output: Value = serde_json::from_str(&large.content[1].as_text().unwrap().text).unwrap();
    let saved = output["path"].as_str().unwrap();
    let mut file = std::fs::File::open(saved).unwrap();
    let mut piece = vec![0; 65536];
    for _ in 0..480 {
        file.read_exact(&mut piece).unwrap();
        assert!(piece.iter().all(|&byte| byte == 71));
    }
    assert_eq!(file.read(&mut piece).unwrap(), 0);
    let uploaded = room_portal_fixture::call_tool(
        &client,
        "upload_attachment",
        json!({"path":"managed-large.bin","content_type":"application/octet-stream"}),
    )
    .await;
    assert_ne!(uploaded.is_error, Some(true), "{uploaded:?}");
    let uploaded: Value =
        serde_json::from_str(&uploaded.content[0].as_text().unwrap().text).unwrap();
    assert_eq!(uploaded["size"], 30 * 1024 * 1024);
    Box::pin(crate::delegated_pin::verify(
        &client, store, delegate, event_id, None, events, owner,
    ))
    .await
    .unwrap();
    client
        .cancel()
        .await
        .unwrap_or_else(|error| panic!("close managed MCP: {error}"));
    uploaded["id"].as_str().unwrap().to_owned()
}

pub(super) async fn publish_with_file(endpoint: &str, token: &str, id: &str) -> String {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
        ))
        .await
        .unwrap();
    let read = room_portal_fixture::call_tool(&client, "read_discussion", json!({})).await;
    let result = room_portal_fixture::call_tool(
        &client,
        "publish_message",
        json!({"content":"first room answer","attachment_ids":[id]}),
    )
    .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let view = read.content[0].as_text().unwrap().text.clone();
    client.cancel().await.unwrap();
    view
}

pub(super) async fn verify_read_only_files(endpoint: &str, token: &str, id: &str) {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
        ))
        .await
        .unwrap();
    room_portal_fixture::call_tool(&client, "read_discussion", json!({})).await;
    let read =
        room_portal_fixture::call_tool(&client, "read_attachment", json!({"attachment_id":id}))
            .await;
    assert_eq!(read.is_error, Some(true));
    let refusal = &read.content[0].as_text().unwrap().text;
    assert!(
        refusal.contains("attachment_too_large")
            && refusal.contains("31457280")
            && refusal.contains("10485760")
    );
    let upload = room_portal_fixture::call_tool(
        &client,
        "upload_attachment",
        json!({"path":"managed-large.bin"}),
    )
    .await;
    assert_eq!(upload.is_error, Some(true));
    assert!(
        upload.content[0]
            .as_text()
            .unwrap()
            .text
            .contains("permission_denied")
    );
    client.cancel().await.unwrap();
}
