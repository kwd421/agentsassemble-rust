use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ParticipantStatus,
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
    verify_departures(ParticipantStatus::Left).await
}

#[tokio::test]
async fn repeated_kicks_preserve_history_without_growing_required_socket_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    verify_departures(ParticipantStatus::Kicked).await
}

#[tokio::test]
async fn repeated_exports_preserve_history_without_growing_required_socket_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    verify_departures(ParticipantStatus::Exported).await
}

#[tokio::test]
async fn expired_connector_memberships_do_not_grow_empty_resumed_snapshot_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    verify_departures(ParticipantStatus::Joined).await
}

async fn verify_departures(status: ParticipantStatus) -> Result<(), Box<dyn std::error::Error>> {
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
        let now = if status == ParticipantStatus::Joined {
            chrono::Utc::now() - chrono::Duration::days(1_001 - index)
        } else {
            chrono::Utc::now()
        };
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
        if index == 999 && status != ParticipantStatus::Joined {
            store
                .execute_message_with_turn(
                    admission.authorization.principal(),
                    "preserved-departed-message",
                    "message.send",
                    &json!({"content":"Historical message remains readable"}),
                )
                .await?;
        }
        depart(&store, &admission.authorization, status).await?;
    }
    let durable = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        durable
            .participants
            .iter()
            .filter(|p| p.status == status && p.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID)
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
    let receipt = socket
        .subscribe(if status == ParticipantStatus::Joined {
            durable.last_seq
        } else {
            0
        })
        .await;
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
    if status == ParticipantStatus::Joined {
        assert_eq!(snapshot["events"], json!([]));
    } else {
        let historical = snapshot["events"]
            .as_array()
            .ok_or("events missing")?
            .iter()
            .find(|event| event["content"] == "Historical message remains readable")
            .ok_or("departed author's history lost")?;
        assert_eq!(historical["display_name"], "Departing participant 999");
    }
    socket.close().await;
    server.stop().await;
    Ok(())
}

async fn depart(
    store: &agentsassemble_persistence::SqliteStore,
    admission: &agentsassemble_persistence::ConnectorSessionAuthorization,
    status: ParticipantStatus,
) -> Result<(), Box<dyn std::error::Error>> {
    if status == ParticipantStatus::Joined {
        return Ok(());
    }
    let request_id = Uuid::new_v4().to_string();
    if status == ParticipantStatus::Left {
        store
            .leave_connector_session(admission, &request_id)
            .await?;
    } else {
        let principal = AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Local operator".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        store
            .execute_participant_removal(
                agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&principal),
                &request_id,
                if status == ParticipantStatus::Kicked {
                    "participant.kick"
                } else {
                    "participant.export"
                },
                &json!({"participant_id": admission.principal().participant_id}),
            )
            .await?;
    }
    Ok(())
}
