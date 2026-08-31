use std::time::Duration;

use agentsassemble_domain::InviteScope;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpStream;
use tokio_tungstenite::MaybeTlsStream;

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod subscription_proof;
}

use support::{
    human_invite::{
        HOST_TOKEN, canonical_session_token, fixture, join, open_session_socket, start,
    },
    local_socket::connect,
    subscription_proof::AuthenticatedTestSocket,
};

type BoundarySocket = AuthenticatedTestSocket<MaybeTlsStream<TcpStream>>;

#[tokio::test]
async fn local_socket_edits_deletes_and_replays_one_exact_sequenced_mutation() {
    let (store, _) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let mut socket = open_local_socket(&server.base_url).await;

    send_command(
        &mut socket,
        "local-message-create",
        "message.send",
        json!({"content": "original local message"}),
    )
    .await;
    let (created_ack, created_event) = receive_commit(&mut socket).await;
    assert_commit(
        &created_ack,
        &created_event,
        "local-message-create",
        "message.send",
        "message_final",
    );
    let message_id = created_ack["result"]["event"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("message ACK omitted its event identity"))
        .to_owned();

    send_command(
        &mut socket,
        "local-message-edit",
        "message.edit",
        json!({"event_id": message_id, "content": "edited local message"}),
    )
    .await;
    let (edit_ack, edit_event) = receive_commit(&mut socket).await;
    assert_commit(
        &edit_ack,
        &edit_event,
        "local-message-edit",
        "message.edit",
        "message_updated",
    );
    assert_eq!(
        edit_ack["result"]["message"]["content"],
        "edited local message"
    );
    assert_eq!(edit_ack["result"]["event"]["target_event_id"], message_id);

    send_command(
        &mut socket,
        "local-message-edit",
        "message.edit",
        json!({"event_id": message_id, "content": "edited local message"}),
    )
    .await;
    let replay = socket.receive_json().await;
    assert_eq!(replay["op"], "ack");
    assert_eq!(replay["action"], "message.edit");
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], edit_ack["result"]);
    assert!(socket.has_no_frame_for(Duration::from_millis(100)).await);

    send_command(
        &mut socket,
        "local-message-delete",
        "message.delete",
        json!({"event_id": message_id}),
    )
    .await;
    let (delete_ack, delete_event) = receive_commit(&mut socket).await;
    assert_commit(
        &delete_ack,
        &delete_event,
        "local-message-delete",
        "message.delete",
        "message_deleted",
    );
    assert_eq!(delete_ack["result"]["message"]["message_deleted"], true);
    assert_eq!(delete_ack["result"]["target_event_id"], message_id);
    socket.close().await;

    let mut reload = connect(&server.base_url, HOST_TOKEN, "general").await;
    assert_eq!(reload.subscribe(0).await["op"], "subscribed");
    let snapshot = reload.receive_json().await;
    assert_eq!(snapshot["op"], "snapshot");
    let tombstone = snapshot["events"]
        .as_array()
        .and_then(|events| events.iter().find(|event| event["id"] == message_id))
        .unwrap_or_else(|| panic!("reloaded snapshot omitted the deleted message"));
    assert_eq!(tombstone["type"], "message_final");
    assert_eq!(tombstone["content"], "");
    assert_eq!(tombstone["message_deleted"], true);
    reload.close().await;
    server.stop().await;
}

#[tokio::test]
async fn admitted_read_write_socket_mutates_only_its_own_message() {
    let (store, credentials) = fixture(InviteScope::ReadWrite).await;
    let server = start(store).await;
    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x61; 32])),
        "113e4567-e89b-12d3-a456-426614174001",
        "Mutation Writer",
        "",
    )
    .await;
    let mut socket = open_session_socket(
        &client,
        &server.base_url,
        canonical_session_token(&admitted),
    )
    .await;

    send_command(
        &mut socket,
        "remote-message-create",
        "message.send",
        json!({"content": "remote original"}),
    )
    .await;
    let (created_ack, created_event) = receive_commit(&mut socket).await;
    assert_commit(
        &created_ack,
        &created_event,
        "remote-message-create",
        "message.send",
        "message_final",
    );
    let message_id = created_ack["result"]["event"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("remote message ACK omitted its identity"))
        .to_owned();

    send_command(
        &mut socket,
        "remote-message-edit",
        "message.edit",
        json!({"event_id": message_id, "content": "remote edited"}),
    )
    .await;
    let (edit_ack, edit_event) = receive_commit(&mut socket).await;
    assert_commit(
        &edit_ack,
        &edit_event,
        "remote-message-edit",
        "message.edit",
        "message_updated",
    );
    assert_eq!(edit_ack["result"]["message"]["content"], "remote edited");

    send_command(
        &mut socket,
        "remote-message-delete",
        "message.delete",
        json!({"event_id": message_id}),
    )
    .await;
    let (delete_ack, delete_event) = receive_commit(&mut socket).await;
    assert_commit(
        &delete_ack,
        &delete_event,
        "remote-message-delete",
        "message.delete",
        "message_deleted",
    );
    assert_eq!(delete_ack["result"]["message"]["message_deleted"], true);
    socket.close().await;
    server.stop().await;
}

#[tokio::test]
async fn admitted_read_only_socket_rejects_message_mutations_before_dispatch() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let mut local = open_local_socket(&server.base_url).await;
    send_command(
        &mut local,
        "read-only-target-create",
        "message.send",
        json!({"content": "operator-owned target"}),
    )
    .await;
    let (created_ack, _) = receive_commit(&mut local).await;
    let message_id = created_ack["result"]["event"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("target message ACK omitted its identity"))
        .to_owned();

    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x62; 32])),
        "213e4567-e89b-12d3-a456-426614174002",
        "Mutation Reader",
        "",
    )
    .await;
    let mut socket = open_session_socket(
        &client,
        &server.base_url,
        canonical_session_token(&admitted),
    )
    .await;

    for (request_id, action, payload) in [
        (
            "read-only-message-edit",
            "message.edit",
            json!({"event_id": message_id, "content": "forbidden edit"}),
        ),
        (
            "read-only-message-delete",
            "message.delete",
            json!({"event_id": message_id}),
        ),
    ] {
        send_command(&mut socket, request_id, action, payload).await;
        let nack = socket.receive_json().await;
        assert_eq!(nack["op"], "nack");
        assert_eq!(nack["request_id"], request_id);
        assert_eq!(nack["action"], action);
        assert_eq!(nack["resolution"], "rejected");
        assert_eq!(nack["error"]["code"], "permission_denied");
        assert!(socket.has_no_frame_for(Duration::from_millis(50)).await);
    }

    socket.close().await;
    local.close().await;
    server.stop().await;
}

#[tokio::test]
async fn stale_human_session_cannot_mutate_on_an_unnotified_live_socket() {
    let (store, credentials) = fixture(InviteScope::ReadWrite).await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x63; 32])),
        "313e4567-e89b-12d3-a456-426614174003",
        "Stale Mutation Writer",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);
    let mut socket = open_session_socket(&client, &server.base_url, session_token).await;
    send_command(
        &mut socket,
        "stale-target-create",
        "message.send",
        json!({"content": "soon stale"}),
    )
    .await;
    let (created_ack, _) = receive_commit(&mut socket).await;
    let message_id = created_ack["result"]["event"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("stale target ACK omitted its identity"))
        .to_owned();

    let fingerprint: [u8; 32] = Sha256::digest(session_token.as_bytes()).into();
    let authorization = store
        .authorize_human_session(&fingerprint)
        .await
        .unwrap_or_else(|error| panic!("authorize controlled stale session: {error}"));
    let revoked = store
        .execute_human_session_participant_leave(
            &authorization,
            "313e4567-e89b-12d3-a456-426614174004",
            &json!({}),
        )
        .await
        .unwrap_or_else(|error| panic!("commit controlled stale session: {error}"));
    assert_eq!(revoked.revoked_session_fingerprints.len(), 1);

    send_command(
        &mut socket,
        "stale-message-edit",
        "message.edit",
        json!({"event_id": message_id, "content": "must not commit"}),
    )
    .await;
    assert!(socket.wait_closed().await);
    let target = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("read target after stale mutation attempt: {error}"))
        .events
        .into_iter()
        .find(|event| event.id == message_id)
        .unwrap_or_else(|| panic!("stale mutation target disappeared"));
    assert_eq!(target.content.as_deref(), Some("soon stale"));
    assert!(!target.extra.contains_key("edited_at"));
    server.stop().await;
}

#[tokio::test]
async fn deleted_vote_has_no_summary_after_its_mutation_event() {
    let (store, _) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let mut socket = open_local_socket(&server.base_url).await;
    send_command(
        &mut socket,
        "vote-target-create",
        "message.send",
        json!({
            "kind": "vote",
            "vote_question": "Delete this vote?",
            "vote_options": ["Yes", "No"],
        }),
    )
    .await;
    let (created_ack, _) = receive_commit(&mut socket).await;
    let vote_id = created_ack["result"]["event"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("vote ACK omitted its identity"))
        .to_owned();

    send_command(
        &mut socket,
        "vote-target-delete",
        "message.delete",
        json!({"event_id": vote_id}),
    )
    .await;
    let (deleted_ack, deleted_event) = receive_commit(&mut socket).await;
    assert_commit(
        &deleted_ack,
        &deleted_event,
        "vote-target-delete",
        "message.delete",
        "message_deleted",
    );

    send_command(
        &mut socket,
        "deleted-vote-summary",
        "room.vote.summary",
        json!({"vote_id": vote_id}),
    )
    .await;
    let rejected = socket.receive_json().await;
    assert_eq!(rejected["op"], "nack");
    assert_eq!(rejected["resolution"], "rejected");
    assert_eq!(rejected["error"]["code"], "vote_not_found");
    socket.close().await;
    server.stop().await;
}

async fn open_local_socket(base_url: &str) -> BoundarySocket {
    let mut socket = connect(base_url, HOST_TOKEN, "general").await;
    let subscribed = socket.subscribe(0).await;
    assert_eq!(subscribed["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    socket
}

async fn send_command(socket: &mut BoundarySocket, request_id: &str, action: &str, payload: Value) {
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": request_id,
            "action": action,
            "payload": payload,
        }))
        .await;
}

async fn receive_commit(socket: &mut BoundarySocket) -> (Value, Value) {
    let mut ack = None;
    let mut event = None;
    for _ in 0..2 {
        let frame = socket.receive_json().await;
        match frame["op"].as_str() {
            Some("ack") => ack = Some(frame),
            Some("event") => event = Some(frame),
            operation => panic!("unexpected mutation frame: {operation:?}"),
        }
    }
    (
        ack.unwrap_or_else(|| panic!("mutation command omitted ACK")),
        event.unwrap_or_else(|| panic!("mutation command omitted event publication")),
    )
}

fn assert_commit(ack: &Value, event: &Value, request_id: &str, action: &str, event_type: &str) {
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["request_id"], request_id);
    assert_eq!(ack["action"], action);
    assert_eq!(ack["resolution"], "committed");
    assert!(ack.get("deduplicated").is_none());
    assert_eq!(event["op"], "event");
    assert_eq!(event["events"][0]["type"], event_type);
    assert_eq!(ack["result"]["event_seq"], event["events"][0]["seq"]);
    assert_eq!(ack["result"]["event"]["id"], event["events"][0]["id"]);
}
