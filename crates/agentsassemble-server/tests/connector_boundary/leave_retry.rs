use super::{InviteScope, Uuid, fixture, human_invite, json, lossy_http, mcp};
use rmcp::model::CallToolRequestParams;
use std::time::Duration;

#[tokio::test]
async fn lost_connector_leave_recovers_exact_receipt_and_releases_stdio_slot()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay =
        lossy_http::LossyHttpRelay::start(&server.base_url, &["/api/room-connector/command"])
            .await?;
    let (mut child, client) = mcp::start_cli().await?;
    let mut invitations = Vec::new();
    for _ in 0..2 {
        let invite = store
            .create_connector_invite(
                &manager,
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                chrono::Utc::now(),
            )
            .await?;
        invitations.push(
            json!({"invite_url":format!("{}/join?token={}",relay.base_url,invite.invite_bearer)}),
        );
    }
    let joined = mcp::call(&client, "room_join", invitations[0].clone()).await;
    let failed = client
        .call_tool(CallToolRequestParams::new("room_leave"))
        .await?;
    assert_eq!(failed.is_error, Some(true));
    let snapshot = store.snapshot("general", 0, 200).await?;
    let committed = snapshot
        .events
        .iter()
        .find(|event| event.event_type == "participant_left")
        .ok_or("committed leave missing")?;
    assert_eq!(
        committed.participant_id.as_deref(),
        joined["participant_id"].as_str()
    );
    let recovered = mcp::call(&client, "room_leave", json!({})).await;
    assert_eq!(recovered["resolution"], "committed");
    assert_eq!(recovered["deduplicated"], true);
    assert_eq!(recovered["result"]["event"]["id"], committed.id);
    assert_eq!(recovered["result"]["event_seq"], committed.seq);
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "participant_left")
            .count(),
        1
    );
    let denied = client
        .call_tool(
            CallToolRequestParams::new("room_read").with_arguments(
                json!({"connection_id":joined["connection_id"]})
                    .as_object()
                    .ok_or("args")?
                    .clone(),
            ),
        )
        .await?;
    assert_eq!(denied.is_error, Some(true));
    let next = mcp::call(&client, "room_join", invitations[1].clone()).await;
    assert_ne!(next["participant_id"], joined["participant_id"]);
    assert_ne!(next["connection_id"], joined["connection_id"]);
    mcp::call(&client, "room_read", json!({})).await;
    mcp::call(&client, "room_leave", json!({})).await;
    client.cancel().await?;
    assert!(
        tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await??
            .success()
    );
    relay.stop().await?;
    server.stop().await;
    Ok(())
}
