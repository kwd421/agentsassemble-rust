use agentsassemble_domain::InviteScope;
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::{human_invite::fixture, local_socket::connect};

#[tokio::test]
async fn settings_update_and_replay_bind_the_exact_durable_event_sequence() {
    let (store, _) = fixture(InviteScope::ReadOnly).await;
    let server = support::human_invite::start(store).await;
    let mut socket = connect(&server.base_url, server.state(), "general").await;
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    let snapshot = socket.receive_json().await;
    let revision = snapshot["room_settings"]["settings_revision"]
        .as_str()
        .unwrap_or_else(|| panic!("snapshot omitted the settings revision"));
    let command = json!({
        "op": "command",
        "request_id": "settings-boundary-1",
        "action": "room.settings.update",
        "payload": {
            "expected_revision": revision,
            "topic": "Exact settings event sequence",
        },
    });

    socket.send_json(&command).await;
    let (ack, event) = receive_commit(&mut socket).await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["action"], "room.settings.update");
    assert_eq!(ack["resolution"], "committed");
    assert!(ack.get("deduplicated").is_none());
    assert_eq!(event["op"], "event");
    assert_eq!(event["events"][0]["type"], "room_settings_updated");
    assert_eq!(ack["result"]["event_seq"], event["events"][0]["seq"]);
    assert_eq!(ack["result"]["event"], event["events"][0]);

    socket.send_json(&command).await;
    let replay = socket.receive_json().await;
    assert_eq!(replay["op"], "ack");
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], ack["result"]);
    socket.close().await;
    server.stop().await;
}

async fn receive_commit<S>(
    socket: &mut support::room_socket_peer::RoomSocketPeer<S>,
) -> (Value, Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    match (first["op"].as_str(), second["op"].as_str()) {
        (Some("ack"), Some("event")) => (first, second),
        (Some("event"), Some("ack")) => (second, first),
        operations => panic!("unexpected settings frames: {operations:?}"),
    }
}
