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
    let invitation = invite(&store, &manager, &relay.base_url).await?;
    let joined = mcp::call(&client, "room_join", invitation).await;
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
    archive(&store, &server, true).await?;
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
    archive(&store, &server, false).await?;
    let invitation = invite(&store, &manager, &relay.base_url).await?;
    let next = mcp::call(&client, "room_join", invitation).await;
    assert_ne!(next["participant_id"], joined["participant_id"]);
    assert_ne!(next["connection_id"], joined["connection_id"]);
    super::mcp_remote::rejected(&client, "room_leave", json!({}), "connection_id_required").await;
    let original = mcp::call(
        &client,
        "room_leave",
        json!({"connection_id":joined["connection_id"]}),
    )
    .await;
    assert_eq!(original, recovered);
    mcp::call(&client, "room_read", json!({})).await;
    mcp::call(
        &client,
        "room_leave",
        json!({"connection_id":next["connection_id"]}),
    )
    .await;
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

async fn invite(
    store: &agentsassemble_persistence::SqliteStore,
    manager: &agentsassemble_persistence::RoomManagerAuthority,
    base_url: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let invite = store
        .create_connector_invite(
            manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    Ok(json!({"invite_url":format!("{base_url}/join?token={}",invite.invite_bearer)}))
}

async fn archive(
    store: &agentsassemble_persistence::SqliteStore,
    server: &human_invite::RunningServer,
    archived: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let authority = store.local_bootstrap_status().await?;
    let room_uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let ticket = server
        .state()
        .tickets
        .issue_server_operator(agentsassemble_domain::LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let result: serde_json::Value = reqwest::Client::new()
        .post(format!("{}/api/rooms/lifecycle", server.base_url))
        .bearer_auth(ticket)
        .json(&json!({"server_id":authority.server_id,
            "authority_lineage_id":authority.authority_lineage_id,"room_id":"general",
            "request_id":Uuid::new_v4().to_string(),"action":"room.archive",
            "payload":{"room_uid":room_uid,"archived":archived}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    assert_eq!(result["resolution"], "committed");
    Ok(())
}
