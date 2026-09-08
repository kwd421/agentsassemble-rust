use agentsassemble_domain::InviteScope;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::{
    human_invite::{canonical_session_token, fixture, join, open_session_socket, start},
    local_socket::connect,
};

#[tokio::test]
async fn current_channel_socket_owner_serves_native_and_human_scopes() {
    for scope in [InviteScope::ReadWrite, InviteScope::ReadOnly] {
        let (store, credentials) = fixture(scope).await;
        let server = start(store).await;
        let mut native = connect(&server.base_url, server.state(), "general").await;
        assert_eq!(native.subscribe(0).await["op"], "subscribed");
        let snapshot = native.receive_json().await;
        let settings = json!({"op":"command","request_id":"channels","action":"room.settings.update",
            "payload":{"expected_revision":snapshot["room_settings"]["settings_revision"],
                "channels":[{"id":"c0123456789ab","name":"First","type":"text","position":0,"created_at":"2026-09-08T00:00:00Z"}]}});
        native.send_json(&settings).await;
        let (settings_ack, _) = receive_commit(&mut native).await;
        let message_command = json!({"op":"command","request_id":"native-channel-send","action":"channel.message.send",
            "payload":{"channel_id":"c0123456789ab","content":"native channel text"}});
        native.send_json(&message_command).await;
        let (sent, event) = receive_commit(&mut native).await;
        assert_eq!(event["events"][0]["type"], "channel_message_final");
        assert_eq!(event["events"][0]["channel_id"], "c0123456789ab");
        assert_eq!(sent["result"]["event"], event["events"][0]);
        native.send_json(&message_command).await;
        let replay = native.receive_json().await;
        assert_eq!(replay["deduplicated"], true);
        assert_eq!(replay["result"], sent["result"]);

        let client = reqwest::Client::new();
        let admission = join(
            &client,
            &server.base_url,
            credentials.join_code(),
            &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x58; 32])),
            "a23e4567-e89b-12d3-a456-426614174000",
            "Channel Human",
            "",
        )
        .await;
        let mut human = open_session_socket(
            &client,
            &server.base_url,
            canonical_session_token(&admission),
        )
        .await;
        // The admission event belongs to the same native room sequence.
        assert_eq!(native.receive_json().await["op"], "event");
        human
            .send_json(
                &json!({"op":"command","request_id":"human-history","action":"channel.history",
            "payload":{"channel_id":"c0123456789ab"}}),
            )
            .await;
        let history = human.receive_json().await;
        assert_eq!(history["op"], "ack");
        assert_eq!(history["result"]["events"], json!([event["events"][0]]));
        human
            .send_json(
                &json!({"op":"command","request_id":"human-send","action":"channel.message.send",
            "payload":{"channel_id":"c0123456789ab","content":"human channel text"}}),
            )
            .await;
        if scope == InviteScope::ReadOnly {
            let denied = human.receive_json().await;
            assert_eq!(denied["op"], "nack");
            assert_eq!(denied["error"]["code"], "permission_denied");
        } else {
            let (ack, live) = receive_commit(&mut human).await;
            assert_eq!(ack["result"]["event"], live["events"][0]);
            assert_eq!(live["events"][0]["content"], "human channel text");
            assert_eq!(native.receive_json().await["op"], "event");
        }
        native.send_json(&json!({"op":"command","request_id":"remove-channel","action":"room.settings.update",
            "payload":{"expected_revision":settings_ack["result"]["room_settings"]["settings_revision"],"channels":[]}})).await;
        let (removed, _) = receive_commit(&mut native).await;
        assert_eq!(removed["op"], "ack");
        assert_eq!(human.receive_json().await["op"], "event");
        human
            .send_json(
                &json!({"op":"command","request_id":"removed-history","action":"channel.history",
            "payload":{"channel_id":"c0123456789ab"}}),
            )
            .await;
        assert_eq!(human.receive_json().await["op"], "nack");
        native.send_json(&message_command).await;
        assert_eq!(native.receive_json().await["op"], "nack");
        human.close().await;
        native.close().await;
        server.stop().await;
    }
}

async fn receive_commit<S>(
    socket: &mut support::room_socket_peer::RoomSocketPeer<S>,
) -> (Value, Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let first = socket.receive_json().await;
    assert!(
        matches!(first["op"].as_str(), Some("ack" | "event")),
        "{first}"
    );
    let second = socket.receive_json().await;
    match (first["op"].as_str(), second["op"].as_str()) {
        (Some("ack"), Some("event")) => (first, second),
        (Some("event"), Some("ack")) => (second, first),
        operations => panic!("unexpected channel frames: {operations:?}; {first}; {second}"),
    }
}
