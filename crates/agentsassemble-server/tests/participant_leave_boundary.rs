use agentsassemble_domain::{InviteScope, ParticipantStatus};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::json;

mod support {
    pub mod human_invite;
    pub mod subscription_proof;
}

use support::human_invite::{canonical_session_token, fixture, join, open_session_socket, start};

#[tokio::test]
async fn websocket_leave_acks_once_then_revokes_every_exact_session_socket() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x75; 32])),
        "823e4567-e89b-12d3-a456-426614174000",
        "Leaving Guest",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);
    let participant_id = admitted["agent_id"]
        .as_str()
        .unwrap_or_else(|| panic!("admission omitted participant identity"));
    let mut command_socket = open_session_socket(&client, &server.base_url, session_token).await;
    let mut idle_socket = open_session_socket(&client, &server.base_url, session_token).await;

    command_socket
        .send_json(&json!({
            "op": "command",
            "request_id": "human-socket-leave-1",
            "action": "participant.leave",
            "payload": {},
        }))
        .await;
    let ack = command_socket.receive_json().await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["request_id"], "human-socket-leave-1");
    assert_eq!(ack["action"], "participant.leave");
    assert_eq!(ack["result"]["participant"]["status"], "left");
    assert!(command_socket.wait_closed().await);
    assert!(idle_socket.wait_closed().await);

    let rejected = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange ended room session: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);

    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("read post-leave snapshot: {error}"));
    let participant = snapshot
        .participants
        .iter()
        .find(|participant| participant.participant_id == participant_id)
        .unwrap_or_else(|| panic!("left participant missing from room history"));
    assert_eq!(participant.status, ParticipantStatus::Left);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| {
                event.event_type == "participant_left"
                    && event.participant_id.as_deref() == Some(participant_id)
            })
            .count(),
        1
    );
    server.stop().await;
}

#[tokio::test]
async fn http_leave_rejects_nonempty_contract_then_revokes_the_live_session() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x76; 32])),
        "923e4567-e89b-12d3-a456-426614174000",
        "HTTP Leaving Guest",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);
    let participant_id = admitted["agent_id"]
        .as_str()
        .unwrap_or_else(|| panic!("HTTP admission omitted participant identity"));

    let invalid = client
        .post(format!("{}/api/room-invite/leave", server.base_url))
        .bearer_auth(session_token)
        .json(&json!({"participant_id": participant_id}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send invalid HTTP leave: {error}"));
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
    assert_eq!(
        invalid
            .json::<serde_json::Value>()
            .await
            .unwrap_or_else(|error| panic!("decode invalid HTTP leave: {error}"))["code"],
        "invalid_participant_leave"
    );
    let mut idle_socket = open_session_socket(&client, &server.base_url, session_token).await;

    let left = client
        .post(format!("{}/api/room-invite/leave", server.base_url))
        .bearer_auth(session_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send HTTP leave: {error}"));
    assert_eq!(left.status(), reqwest::StatusCode::OK);
    assert_eq!(left.headers()["cache-control"], "private, no-store");
    let left = left
        .json::<serde_json::Value>()
        .await
        .unwrap_or_else(|error| panic!("decode HTTP leave: {error}"));
    assert_eq!(left, json!({"status": "left", "agent_id": participant_id}));
    assert!(idle_socket.wait_closed().await);

    let rejected = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange HTTP-ended session: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("read HTTP post-leave snapshot: {error}"));
    assert!(snapshot.participants.iter().any(|participant| {
        participant.participant_id == participant_id
            && participant.status == ParticipantStatus::Left
    }));
    server.stop().await;
}
