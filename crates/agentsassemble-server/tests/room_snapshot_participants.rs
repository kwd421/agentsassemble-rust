use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ParticipantStatus,
};
use agentsassemble_server::issue_local_ticket;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use uuid::Uuid;

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}

#[tokio::test]
async fn repeated_departures_preserve_history_without_growing_required_socket_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _) = support::human_invite::fixture(InviteScope::ReadOnly).await;
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let manager = agentsassemble_persistence::RoomManagerAuthority::Local(manager);
    for index in 0..1_000 {
        let now = chrono::Utc::now();
        let invite = store
            .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
            .await?;
        let admission = store
            .admit_connector(
                &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
                &[7; 32],
                Uuid::new_v4(),
                &format!("Departing participant {index}"),
                now,
            )
            .await?;
        if index == 999 {
            store
                .execute_message_with_turn(
                    admission.authorization.principal(),
                    "preserved-departed-message",
                    "message.send",
                    &json!({"content":"Historical message remains readable"}),
                )
                .await?;
        }
        store
            .leave_connector_session(&admission.authorization, &Uuid::new_v4().to_string())
            .await?;
    }
    let durable = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        durable
            .participants
            .iter()
            .filter(|p| p.status == ParticipantStatus::Left)
            .count(),
        1_000
    );
    assert!(
        serde_json::to_vec(&durable.participants)?.len()
            > agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES
    );
    let server = support::human_invite::start(store.clone()).await;
    let ticket = issue_local_ticket(server.state(), "general").await?;
    let (socket, _) = tokio_tungstenite::connect_async(format!(
        "{}/ws?ticket={}",
        server.base_url.replacen("http://", "ws://", 1),
        ticket.ticket,
    ))
    .await?;
    let mut socket = support::room_socket_peer::RoomSocketPeer::new(socket);
    let receipt = socket.subscribe(0).await;
    assert_eq!(receipt["op"], "subscribed", "{receipt}");
    let wire = socket.receive_text().await;
    let snapshot: Value = serde_json::from_str(&wire)?;
    assert_eq!(snapshot["op"], "snapshot");
    assert!(wire.len() <= agentsassemble_protocol::MAX_ROOM_SOCKET_MESSAGE_BYTES);
    let participants = snapshot["participants"]
        .as_array()
        .ok_or("participants missing")?;
    assert_eq!(participants.len(), 1);
    assert_eq!(
        participants[0]["participant_id"],
        LOCAL_OPERATOR_PARTICIPANT_ID
    );
    let historical = snapshot["events"]
        .as_array()
        .ok_or("events missing")?
        .iter()
        .find(|event| event["content"] == "Historical message remains readable")
        .ok_or("departed author's history lost")?;
    assert_eq!(historical["display_name"], "Departing participant 999");
    socket.close().await;
    server.stop().await;
    Ok(())
}
