use super::{InviteScope, Uuid, fixture, human_invite, json};
use agentsassemble_protocol::RoomAction;
use agentsassemble_server::connector_client::RoomConnectorClient;
#[tokio::test]
async fn lost_admission_and_command_responses_recover_the_original_receipts()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay = super::lossy_http::LossyHttpRelay::start(
        &server.base_url,
        &["/api/room-connector/join", "/api/room-connector/command"],
    )
    .await?;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let client = RoomConnectorClient::new(
        &format!("{}/join?token={}", relay.base_url, invite.invite_bearer),
        "Retry AI",
        None,
    )?;
    assert!(client.join().await.is_err());
    client.join().await?;
    let payload = json!({"content":"One committed contribution"});
    assert!(
        client
            .command(RoomAction::MessageSend, payload.clone())
            .await
            .is_err()
    );
    let different = client
        .command(
            RoomAction::MessageSend,
            json!({"content":"Different request"}),
        )
        .await;
    assert!(different.is_err_and(|error| error.code == "previous_connector_command_unresolved"));
    let receipt = client.command(RoomAction::MessageSend, payload).await?;
    assert_eq!(receipt["deduplicated"], true);
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .participants
            .iter()
            .filter(|participant| participant.participant_type == "agent")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("One committed contribution"))
            .count(),
        1
    );
    client
        .command(RoomAction::ParticipantLeave, json!({}))
        .await?;
    client.close();
    relay.stop().await?;
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn admitted_initial_read_loss_can_leave_without_a_successful_read()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, manager) = fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay = super::lossy_http::LossyHttpRelay::start(
        &server.base_url,
        &["/api/room-connector/read", "/api/room-connector/command"],
    )
    .await?;
    let invite = store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            chrono::Utc::now(),
        )
        .await?;
    let client = RoomConnectorClient::new(
        &format!("{}/join?token={}", relay.base_url, invite.invite_bearer),
        "Admitted read loss",
        None,
    )?;
    assert!(client.join().await.is_err());
    let snapshot = store.snapshot("general", 0, 200).await?;
    let participant = snapshot
        .participants
        .iter()
        .find(|entry| entry.display_name == "Admitted read loss")
        .ok_or("successful admission missing")?
        .participant_id
        .clone();
    // Admission alone must not enable ordinary commands before the initial view.
    assert!(
        client
            .command(RoomAction::MessageSend, json!({"content":"not ready"}))
            .await
            .is_err_and(|error| error.code == "connector_not_joined")
    );
    let request_id = Uuid::new_v4();
    assert!(
        client
            .command_with_request_id(request_id, RoomAction::ParticipantLeave, json!({}))
            .await
            .is_err()
    );
    let receipt = client
        .command_with_request_id(request_id, RoomAction::ParticipantLeave, json!({}))
        .await?;
    assert_eq!(receipt["resolution"], "committed");
    assert_eq!(receipt["deduplicated"], true);
    assert_eq!(
        client
            .command_with_request_id(request_id, RoomAction::ParticipantLeave, json!({}))
            .await?,
        receipt
    );
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "participant_left"
                && event.participant_id.as_deref() == Some(participant.as_str()))
            .count(),
        1
    );
    assert_ne!(
        snapshot
            .participants
            .iter()
            .find(|entry| entry.participant_id == participant)
            .ok_or("participant history missing")?
            .status,
        agentsassemble_domain::ParticipantStatus::Joined
    );
    relay.stop().await?;
    server.stop().await;
    Ok(())
}
