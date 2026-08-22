use std::{sync::Arc, time::Duration};

use agentsassemble_domain::{Participant, ParticipantStatus, Room, RoomSettings};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_server::{AppState, RoomRuntime, TicketStore, serve};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

struct RunningServer {
    base_url: String,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("server task join: {error}"));
    }
}

#[tokio::test]
async fn external_client_recovers_committed_command_after_restart() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create temporary data root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open first store: {error}"));
    bootstrap(&store).await;
    let first_server = start(store).await;
    let mut first_socket = connect(&first_server.base_url).await;
    subscribe(&mut first_socket, 0).await;
    let initial = receive_json(&mut first_socket).await;
    assert_eq!(initial["op"], "snapshot");
    assert_eq!(initial["last_seq"], 0);
    send_command(&mut first_socket).await;
    let mut committed_event = None;
    let mut committed_ack = None;
    for _ in 0..2 {
        let frame = receive_json(&mut first_socket).await;
        match frame["op"].as_str() {
            Some("event") => committed_event = Some(frame),
            Some("ack") => committed_ack = Some(frame),
            operation => panic!("unexpected first-server frame: {operation:?}"),
        }
    }
    let event = committed_event.unwrap_or_else(|| panic!("event frame was not delivered"));
    let ack = committed_ack.unwrap_or_else(|| panic!("ACK frame was not delivered"));
    assert_eq!(event["events"][0]["seq"], 1);
    assert_eq!(ack["result"]["event_seq"], 1);
    assert_eq!(ack["result"]["event"]["id"], event["events"][0]["id"]);
    first_socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close first socket: {error}"));
    first_server.stop().await;

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen store: {error}"));
    let second_server = start(reopened).await;
    let mut second_socket = connect(&second_server.base_url).await;
    subscribe(&mut second_socket, 0).await;
    let recovered = receive_json(&mut second_socket).await;
    assert_eq!(recovered["op"], "snapshot");
    assert_eq!(recovered["last_seq"], 1);
    assert_eq!(recovered["events"][0]["content"], "boundary hello");
    send_command(&mut second_socket).await;
    let retry = receive_json(&mut second_socket).await;
    assert_eq!(retry["op"], "ack");
    assert_eq!(retry["deduplicated"], true);
    assert_eq!(retry["result"]["event_seq"], 1);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), second_socket.next())
            .await
            .is_err()
    );
    second_server.stop().await;
}

async fn start(store: SqliteStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind test runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read test runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState {
        rooms: RoomRuntime::new(store.clone()),
        store,
        tickets: TicketStore::new(Duration::from_secs(30), 16),
        host_token: Arc::from(""),
    };
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve test runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        cancellation,
        task,
    }
}

async fn connect(
    base_url: &str,
) -> WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let response = Client::new()
        .post(format!("{base_url}/api/ws-ticket"))
        .json(&json!({"meeting_id": "general"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ticket: {error}"));
    assert!(response.status().is_success());
    let grant: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode ticket: {error}"));
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no ticket"));
    let url = format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect WebSocket: {error}"))
        .0
}

async fn subscribe<S>(socket: &mut WebSocketStream<S>, cursor: i64)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({"op": "subscribe", "streams": ["room_events"], "resume_from_seq": cursor})
                .to_string()
                .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send subscription: {error}"));
}

async fn send_command<S>(socket: &mut WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({
                "op": "command",
                "request_id": "boundary-request-1",
                "action": "message.send",
                "payload": {"content": "boundary hello"}
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send command: {error}"));
}

async fn receive_json<S>(socket: &mut WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for WebSocket frame"))
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    serde_json::from_slice(&message.into_data())
        .unwrap_or_else(|error| panic!("decode WebSocket JSON: {error}"))
}

async fn bootstrap(store: &SqliteStore) {
    let now = Utc::now();
    let room = Room::new("general".to_owned(), "General".to_owned(), now);
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: "host".to_owned(),
        display_name: "Host".to_owned(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: "host".to_owned(),
        owner_id: String::new(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    store
        .bootstrap_room(
            &room,
            &RoomSettings::defaults("General".to_owned()),
            &participant,
        )
        .await
        .unwrap_or_else(|error| panic!("bootstrap boundary room: {error}"));
}
