use agentsassemble_domain::InviteScope;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::human_invite::{canonical_session_token, fixture, join, open_session_socket, start};

#[tokio::test]
async fn manager_kick_revokes_live_guest_sockets_and_session_ticket_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x77; 32])),
        "a23e4567-e89b-12d3-a456-426614174000",
        "Removed Guest",
        "",
    )
    .await;
    let token = canonical_session_token(&admitted);
    let mut guest = open_session_socket(&client, &server.base_url, token).await;
    let mut second = open_session_socket(&client, &server.base_url, token).await;
    let mut manager =
        support::local_socket::connect(&server.base_url, server.state(), "general").await;
    assert_eq!(manager.subscribe(0).await["op"], "subscribed");
    assert_eq!(manager.receive_json().await["op"], "snapshot");
    manager
        .send_json(&json!({
            "op": "command",
            "request_id": "manager-kick-guest",
            "action": "participant.kick",
            "payload": {"participant_id": admitted["agent_id"]},
        }))
        .await;
    let ack = manager
        .receive_json_with_timeout(Duration::from_secs(5))
        .await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["result"]["participant"]["status"], "kicked");
    assert_eq!(ack["result"]["revoked_sessions"], 1);
    assert!(tokio::time::timeout(Duration::from_secs(5), guest.wait_closed()).await?);
    assert!(tokio::time::timeout(Duration::from_secs(5), second.wait_closed()).await?);
    let rejected = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .bearer_auth(token)
        .send()
        .await?;
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
    manager.close().await;
    server.stop().await;
    Ok(())
}
