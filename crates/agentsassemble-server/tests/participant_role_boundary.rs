use agentsassemble_domain::{InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID};
use serde_json::{Value, json};

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::{human_invite::fixture, local_socket::connect};

#[tokio::test]
async fn role_update_and_replay_bind_the_exact_durable_event_sequence() {
    let (store, _) = fixture(InviteScope::ReadOnly).await;
    let server = support::human_invite::start(store).await;
    let mut socket = connect(&server.base_url, server.state(), "general").await;
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    let command = json!({
        "op": "command",
        "request_id": "participant-role-boundary-1",
        "action": "participant.role.update",
        "payload": {
            "participant_id": LOCAL_OPERATOR_PARTICIPANT_ID,
            "role": "reviewer",
        },
    });

    socket.send_json(&command).await;
    let (ack, published) = receive_commit(&mut socket).await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["action"], "participant.role.update");
    assert_eq!(ack["resolution"], "committed");
    assert!(ack.get("deduplicated").is_none());
    assert_eq!(ack["result"]["participant"]["role"], "reviewer");
    assert_eq!(published["events"][0]["type"], "participant_updated");
    assert_eq!(ack["result"]["event_seq"], published["events"][0]["seq"]);
    assert_eq!(ack["result"]["event"], published["events"][0]);

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
        operations => panic!("unexpected role-update frames: {operations:?}"),
    }
}
