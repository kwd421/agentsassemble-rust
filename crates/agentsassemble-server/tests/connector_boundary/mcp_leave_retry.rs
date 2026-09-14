use super::{
    InviteScope, Uuid, fixture, human_invite, json, lossy_http::LossyHttpRelay, mcp::call,
    mcp_remote::RemoteMcp,
};
use rmcp::{ServiceExt, model::CallToolRequestParams, transport::StreamableHttpClientTransport};

#[tokio::test]
async fn lost_mcp_leave_response_retains_receipt_until_explicit_release()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let remote = RemoteMcp::start(&server.base_url).await?;
    let client = ()
        .serve(StreamableHttpClientTransport::from_uri(
            remote.endpoint.clone(),
        ))
        .await?;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let mut invitation =
        json!({"invite_url":format!("{}/join?token={}",server.base_url,invite.invite_bearer)});
    let prepared = call(&client, "room_join", invitation.clone()).await;
    invitation["connection_id"] = prepared["connection_id"].clone();
    let joined = call(&client, "room_join", invitation.clone()).await;
    let args = json!({"connection_id":joined["connection_id"]});
    let relay = LossyHttpRelay::start(
        remote
            .endpoint
            .strip_suffix("/mcp")
            .ok_or("MCP path missing")?,
        &["/mcp"],
    )
    .await?;
    let lost = reqwest::Client::new().post(format!("{}/mcp",relay.base_url))
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "2025-03-26")
        .json(&json!({"jsonrpc":"2.0","id":42,"method":"tools/call","params":{"name":"room_leave","arguments":args}}))
        .send().await?.error_for_status()?.text().await?;
    assert_eq!(lost, "{");
    let before = store.snapshot("general", 0, 200).await?;
    let terminal = before
        .events
        .iter()
        .find(|event| {
            event.event_type == "participant_left"
                && event.actor.participant_id
                    == joined["participant_id"].as_str().unwrap_or_default()
        })
        .ok_or("leave did not commit")?;
    let (first, second) = tokio::join!(
        call(&client, "room_leave", args.clone()),
        call(&client, "room_leave", args.clone())
    );
    assert_eq!(first, second);
    assert_eq!(first["result"]["event"]["id"], terminal.id);
    let denied = client
        .call_tool(
            CallToolRequestParams::new("room_read")
                .with_arguments(args.as_object().ok_or("arguments missing")?.clone()),
        )
        .await?;
    assert_eq!(denied.is_error, Some(true));
    let after = store.snapshot("general", 0, 200).await?;
    assert_eq!(before.last_seq, after.last_seq);
    invitation
        .as_object_mut()
        .ok_or("invitation missing")?
        .remove("connection_id");
    fill_receipt_capacity(&client, &invitation).await?;
    let released = call(
        &client,
        "room_leave",
        json!({"connection_id":joined["connection_id"],"release_receipt":true}),
    )
    .await;
    assert_eq!(released["status"], "receipt_released");
    let gone = client
        .call_tool(
            CallToolRequestParams::new("room_leave")
                .with_arguments(args.as_object().ok_or("arguments missing")?.clone()),
        )
        .await?;
    assert_eq!(gone.is_error, Some(true));
    assert_eq!(
        call(&client, "room_join", invitation).await["status"],
        "connection_prepared"
    );
    client.cancel().await?;
    relay.stop().await?;
    remote.stop().await?;
    server.stop().await;
    Ok(())
}

async fn fill_receipt_capacity(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
    invitation: &serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    // The completed receipt remains one of the existing 128 registry entries.
    for _ in 0..127 {
        assert_eq!(
            call(client, "room_join", invitation.clone()).await["status"],
            "connection_prepared"
        );
    }
    let full = client
        .call_tool(
            CallToolRequestParams::new("room_join")
                .with_arguments(invitation.as_object().ok_or("invitation missing")?.clone()),
        )
        .await?;
    assert_eq!(full.is_error, Some(true));
    assert!(
        full.content
            .iter()
            .filter_map(|item| item.as_text())
            .any(|item| item
                .text
                .contains("connector_capacity_release_receipt_required"))
    );
    Ok(())
}
