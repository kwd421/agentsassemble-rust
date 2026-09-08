use agentsassemble_domain::{InviteScope, SideChatSnapshot};
use agentsassemble_server::issue_side_chat_read_ticket;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}
use support::{
    human_invite::{
        canonical_session_token, fixture, join, open_session_socket_with_streams, start,
    },
    local_socket::connect,
};

#[tokio::test]
async fn live_side_chat_overlaps_bootstrap_replays_and_stays_out_of_durable_events()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _) = fixture(InviteScope::ReadWrite).await;
    let server = start(store.clone()).await;
    let mut live = connect(&server.base_url, server.state(), "general").await;
    live.send_json(
        &json!({"op":"subscribe","streams":["room_events","side_chat"],"resume_from_seq":0}),
    )
    .await;
    assert_eq!(
        live.receive_json().await["streams"],
        json!(["room_events", "side_chat"])
    );
    let room_snapshot = live.receive_json().await;
    let initial = bootstrap(&server).await?;
    let command = json!({"op":"command","request_id":"side-one","action":"side_chat.send",
        "payload":{"generation":initial.generation,"after_seq":0,"content":"private live text"}});
    live.send_json(&command).await;
    let (ack, update) = receive_commit(&mut live).await;
    assert_eq!(ack["result"]["update"], update["update"]);
    let overlap = bootstrap(&server).await?;
    assert_eq!(overlap.latest_seq, 1);
    assert_eq!(json!(overlap.messages[0]), update["update"]["message"]);
    live.send_json(&command).await;
    let replay = live.receive_json().await;
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], ack["result"]);
    let mut observer = connect(&server.base_url, server.state(), "general").await;
    assert_eq!(observer.subscribe(0).await["op"], "subscribed");
    assert_eq!(
        observer.receive_json().await["last_seq"],
        room_snapshot["last_seq"]
    );
    let next = json!({"op":"command","request_id":"side-two","action":"side_chat.send",
        "payload":{"generation":initial.generation,"after_seq":1,"content":"second"}});
    live.send_json(&next).await;
    let (_, update) = receive_commit(&mut live).await;
    assert_eq!(update["update"]["message"]["seq"], 2);
    observer
        .send_json(&json!({"op":"ping","nonce":"no-side-stream"}))
        .await;
    assert_eq!(observer.receive_json().await["op"], "pong");
    let durable = store.snapshot("general", 0, 100).await?;
    assert_eq!(json!(durable.last_seq), room_snapshot["last_seq"]);
    assert!(!serde_json::to_string(&durable.events)?.contains("private live text"));
    live.close().await;
    let mut reconnected = connect(&server.base_url, server.state(), "general").await;
    reconnected.send_json(&json!({"op":"subscribe","streams":["room_events","side_chat"],"resume_from_seq":durable.last_seq})).await;
    assert_eq!(reconnected.receive_json().await["op"], "subscribed");
    assert_eq!(reconnected.receive_json().await["op"], "snapshot");
    assert_eq!(bootstrap(&server).await?.latest_seq, 2);
    reconnected.send_json(&next).await;
    assert_eq!(reconnected.receive_json().await["deduplicated"], true);
    reconnected.close().await;
    observer.close().await;
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn read_only_human_receives_private_updates_but_cannot_post_and_leave_closes_delivery()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let client = Client::new();
    let admission = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x58; 32])),
        "a23e4567-e89b-12d3-a456-426614174000",
        "Side Reader",
        "",
    )
    .await;
    let token = canonical_session_token(&admission);
    let mut reader = open_session_socket_with_streams(
        &client,
        &server.base_url,
        token,
        &["room_events", "side_chat"],
    )
    .await;
    let initial = bootstrap(&server).await?;
    let command = json!({"op":"command","request_id":"side-scope","action":"side_chat.send",
        "payload":{"generation":initial.generation,"after_seq":0,"content":"read-only cannot post"}});
    reader.send_json(&command).await;
    let denied = reader.receive_json().await;
    assert_eq!(denied["op"], "nack");
    assert_eq!(denied["error"]["code"], "permission_denied");
    let mut native = connect(&server.base_url, server.state(), "general").await;
    assert_eq!(native.subscribe(0).await["op"], "subscribed");
    assert_eq!(native.receive_json().await["op"], "snapshot");
    native.send_json(&command).await;
    assert_eq!(native.receive_json().await["op"], "ack");
    assert_eq!(reader.receive_json().await["op"], "side_chat_updated");
    assert_eq!(
        client
            .post(format!("{}/api/room-invite/leave", server.base_url))
            .bearer_auth(token)
            .json(&json!({}))
            .send()
            .await?
            .status(),
        StatusCode::OK
    );
    assert!(reader.wait_closed().await);
    native.close().await;
    server.stop().await;
    Ok(())
}

async fn bootstrap(
    server: &support::human_invite::RunningServer,
) -> Result<SideChatSnapshot, Box<dyn std::error::Error>> {
    let ticket = issue_side_chat_read_ticket(server.state(), "general").await?;
    let response = Client::new()
        .get(format!("{}/api/side-chat?room_id=general", server.base_url))
        .bearer_auth(ticket.ticket)
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    Ok(response.json().await?)
}

async fn receive_commit<S>(
    socket: &mut support::room_socket_peer::RoomSocketPeer<S>,
) -> (Value, Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let first = socket.receive_json().await;
    assert!(
        matches!(first["op"].as_str(), Some("ack" | "side_chat_updated")),
        "{first}"
    );
    let second = socket.receive_json().await;
    match (first["op"].as_str(), second["op"].as_str()) {
        (Some("ack"), Some("side_chat_updated")) => (first, second),
        (Some("side_chat_updated"), Some("ack")) => (second, first),
        operations => panic!("unexpected private frames: {operations:?}"),
    }
}
