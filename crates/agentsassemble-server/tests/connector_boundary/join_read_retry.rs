use super::{InviteScope, Uuid, fixture, human_invite, json, mcp::call, mcp_remote};
use rmcp::{ServiceExt, transport::StreamableHttpClientTransport};

#[tokio::test]
async fn rejected_rejoin_read_keeps_admitted_connection_available_for_leave()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let remote = mcp_remote::RemoteMcp::start(&server.base_url).await?;
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
    let connection = json!({"connection_id":joined["connection_id"]});
    // Initial join consumes 200 events of the existing 1000-event read budget.
    // Exhaust the remaining budget through real HTTP reads, not a fabricated error.
    for _ in 0..4 {
        call(&client, "room_read", connection.clone()).await;
    }
    mcp_remote::rejected(&client, "room_join", invitation, "room_read_limit").await;
    let receipt = call(&client, "room_leave", connection).await;
    assert_eq!(receipt["resolution"], "committed");
    let snapshot = store.snapshot("general", 0, 200).await?;
    let participant = snapshot
        .participants
        .iter()
        .find(|participant| {
            Some(participant.participant_id.as_str()) == joined["participant_id"].as_str()
        })
        .ok_or("admitted participant missing")?;
    assert_ne!(
        participant.status,
        agentsassemble_domain::ParticipantStatus::Joined
    );
    client.cancel().await?;
    remote.stop().await?;
    server.stop().await;
    Ok(())
}
