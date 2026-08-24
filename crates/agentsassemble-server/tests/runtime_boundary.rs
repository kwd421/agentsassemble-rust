use std::{fmt::Write, path::PathBuf, time::Duration};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

#[path = "support/subscription_proof.rs"]
mod subscription_proof;

use subscription_proof::{
    connection_nonce_for_ticket, expected_subscription_proof, permissions_digest, sha256_hex,
};

const HOST_TOKEN: &str = "boundary-test-host-token-0000000001";
const HOST_CHALLENGE_CONTEXT: &str = "agentsassemble-host-challenge-v1\0";
const HOST_REQUEST_CONTEXT: &str = "agentsassemble-host-ticket-request-v1\0";
const HOST_RESPONSE_CONTEXT: &str = "agentsassemble-host-ticket-response-v1\0";

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
    let unauthenticated = Client::new()
        .post(format!("{}/api/ws-ticket", first_server.base_url))
        .json(&json!({"meeting_id": "general"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unauthenticated ticket: {error}"));
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);
    let wrong_challenge = "a".repeat(64);
    let wrong_proof = expected_host_request_proof(
        "wrong-boundary-host-token-000000000",
        &wrong_challenge,
        "general",
    );
    let wrong_secret = Client::new()
        .post(format!("{}/api/ws-ticket", first_server.base_url))
        .header("x-host-challenge", wrong_challenge)
        .header("x-host-meeting", "general")
        .header("x-host-proof", wrong_proof)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ticket with wrong secret: {error}"));
    assert_eq!(wrong_secret.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert_ticket_challenge_is_single_use(&first_server.base_url).await;
    let mut first_socket = connect(&first_server.base_url).await;
    subscribe(&mut first_socket, 0).await;
    let initial = receive_json(&mut first_socket).await;
    assert_eq!(initial["op"], "snapshot");
    assert_eq!(initial["last_seq"], 1);
    assert_eq!(initial["events"][0]["type"], "room_created");
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
    assert_eq!(event["events"][0]["seq"], 2);
    assert_eq!(ack["result"]["event_seq"], 2);
    assert_eq!(ack["result"]["event"]["id"], event["events"][0]["id"]);
    assert_eq!(event["events"][0]["id"].as_str().map(str::len), Some(36));
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
    assert_eq!(recovered["last_seq"], 2);
    let recovered_message = recovered["events"]
        .as_array()
        .and_then(|events| events.iter().find(|event| event["type"] == "message_final"))
        .unwrap_or_else(|| panic!("recovered snapshot omitted the committed message"));
    assert_eq!(recovered_message["content"], "boundary hello");
    send_command(&mut second_socket).await;
    let retry = receive_json(&mut second_socket).await;
    assert_eq!(retry["op"], "ack");
    assert_eq!(retry["deduplicated"], true);
    assert_eq!(retry["result"]["event_seq"], 2);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), second_socket.next())
            .await
            .is_err()
    );
    let mut cursor_ahead = connect(&second_server.base_url).await;
    let resync = subscribe(&mut cursor_ahead, 50).await;
    assert_eq!(resync["op"], "resync_required");
    assert_eq!(resync["latest_seq"], 2);
    second_server.stop().await;
}

#[tokio::test]
async fn incomplete_http_headers_expire_and_admission_is_bounded() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start(store).await;
    let address = server.base_url.replacen("http://", "", 1);
    let mut sockets = Vec::new();
    for _ in 0..160 {
        let mut socket = TcpStream::connect(&address)
            .await
            .unwrap_or_else(|error| panic!("connect partial HTTP client: {error}"));
        socket
            .write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nX-Slow: ")
            .await
            .unwrap_or_else(|error| panic!("write partial HTTP header: {error}"));
        sockets.push(socket);
    }
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut rejected = 0;
    for socket in &mut sockets {
        let mut byte = [0_u8; 1];
        if matches!(
            tokio::time::timeout(Duration::from_millis(20), socket.read(&mut byte)).await,
            Ok(Ok(0) | Err(_))
        ) {
            rejected += 1;
        }
    }
    assert!(
        rejected > 0,
        "pre-auth HTTP admission did not reject overload"
    );

    tokio::time::sleep(Duration::from_secs(4)).await;
    let mut byte = [0_u8; 1];
    let expired =
        tokio::time::timeout(Duration::from_millis(200), sockets[0].read(&mut byte)).await;
    assert!(
        matches!(expired, Ok(Ok(0) | Err(_))),
        "incomplete HTTP header survived the configured deadline"
    );
    server.stop().await;
}

#[tokio::test]
async fn ticket_auth_and_route_limit_are_checked_before_request_body() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start(store).await;
    let address = server.base_url.replacen("http://", "", 1);

    let unauthorized = header_only_request(
        &address,
        "POST /api/ws-ticket HTTP/1.1\r\nHost: localhost\r\nContent-Length: 1048576\r\n\r\n",
    )
    .await;
    assert!(unauthorized.starts_with("HTTP/1.1 401"));

    let challenge = request_host_challenge(&server.base_url).await;
    let proof = expected_host_request_proof(HOST_TOKEN, &challenge, "general");
    let oversized = header_only_request(
        &address,
        &format!(
            "POST /api/ws-ticket HTTP/1.1\r\nHost: localhost\r\nx-host-challenge: {challenge}\r\nx-host-meeting: general\r\nx-host-proof: {proof}\r\nContent-Length: 1048576\r\n\r\n"
        ),
    )
    .await;
    assert!(oversized.starts_with("HTTP/1.1 413"));
    server.stop().await;
}

#[tokio::test]
async fn snapshot_is_trimmed_to_the_websocket_message_budget() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start(store).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket, 0).await;
    let _ = receive_json(&mut socket).await;
    let content = "x".repeat(12_000);
    for index in 0..32 {
        socket
            .send(Message::Text(
                json!({
                    "op": "command",
                    "request_id": format!("snapshot-budget-{index}"),
                    "action": "message.send",
                    "payload": {"content": content}
                })
                .to_string()
                .into(),
            ))
            .await
            .unwrap_or_else(|error| panic!("send snapshot fixture command: {error}"));
        for _ in 0..2 {
            let _ = receive_json(&mut socket).await;
        }
    }
    socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close fixture socket: {error}"));

    let mut resumed = connect(&server.base_url).await;
    subscribe(&mut resumed, 0).await;
    let snapshot = receive_json(&mut resumed).await;
    let encoded =
        serde_json::to_vec(&snapshot).unwrap_or_else(|error| panic!("re-encode snapshot: {error}"));
    assert!(encoded.len() <= 256 * 1024);
    assert_eq!(snapshot["last_seq"], 33);
    assert_eq!(snapshot["has_more_before"], true);
    assert_eq!(snapshot["resume_gap"], false);
    assert_eq!(snapshot["snapshot_mode"], "initial");
    assert!(snapshot["events"].as_array().is_some_and(|events| {
        !events.is_empty()
            && events.len() < 33
            && events.last().is_some_and(|event| event["seq"] == 33)
    }));
    server.stop().await;
}

#[tokio::test]
async fn static_frontend_has_browser_security_headers() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let frontend = directory.path().join("frontend");
    tokio::fs::create_dir(&frontend)
        .await
        .unwrap_or_else(|error| panic!("create frontend root: {error}"));
    tokio::fs::write(
        frontend.join("index.html"),
        "<!doctype html><title>test</title>",
    )
    .await
    .unwrap_or_else(|error| panic!("write frontend index: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start_with_frontend(store, frontend).await;
    let response = Client::new()
        .get(format!("{}/app/", server.base_url))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request static frontend: {error}"));
    assert!(response.status().is_success());
    assert!(response.headers().contains_key("content-security-policy"));
    assert_eq!(
        response
            .headers()
            .get("x-content-type-options")
            .and_then(|value| value.to_str().ok()),
        Some("nosniff")
    );
    assert_eq!(
        response
            .headers()
            .get("x-frame-options")
            .and_then(|value| value.to_str().ok()),
        Some("DENY")
    );
    assert_eq!(
        response
            .headers()
            .get("connection")
            .and_then(|value| value.to_str().ok()),
        Some("close")
    );
    server.stop().await;
}

#[tokio::test]
async fn authenticated_binary_frame_is_rejected_and_closed() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start(store).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket, 0).await;
    let snapshot = receive_json(&mut socket).await;
    assert_eq!(snapshot["op"], "snapshot");
    socket
        .send(Message::Binary(vec![1, 2, 3].into()))
        .await
        .unwrap_or_else(|error| panic!("send binary frame: {error}"));
    let nack = receive_json(&mut socket).await;
    assert_eq!(nack["error"]["code"], "binary_frame_unsupported");
    let closed = tokio::time::timeout(Duration::from_secs(1), socket.next()).await;
    assert!(matches!(
        closed,
        Ok(None | Some(Ok(Message::Close(_)) | Err(_)))
    ));
    server.stop().await;
}

#[tokio::test]
async fn websocket_snapshot_proves_the_private_ticket_control_channel() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open test store: {error}"));
    bootstrap(&store).await;
    let server = start(store).await;
    let grant = request_ticket(&server.base_url).await;
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket grant is missing a ticket"));
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket grant is missing a proof key"));
    let url = format!(
        "{}/ws?ticket={ticket}",
        server.base_url.replacen("http://", "ws://", 1)
    );
    let mut socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect proved WebSocket: {error}"))
        .0;
    let challenge = "b".repeat(64);
    socket
        .send(Message::Text(
            json!({
                "op": "subscribe",
                "streams": ["room_events"],
                "resume_from_seq": 0,
                "server_challenge": challenge,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send proved subscription: {error}"));
    let receipt = receive_json(&mut socket).await;
    assert_eq!(receipt["op"], "subscribed");
    let received = receipt["proof"]
        .as_str()
        .unwrap_or_else(|| panic!("subscription receipt is missing proof"));
    assert_eq!(received, expected_subscription_proof(proof_key, &receipt));
    assert_eq!(
        receipt["connection_nonce"],
        connection_nonce_for_ticket(ticket)
    );
    let raw_snapshot = receive_text(&mut socket).await;
    let snapshot: Value = serde_json::from_str(&raw_snapshot)
        .unwrap_or_else(|error| panic!("decode proved snapshot: {error}"));
    assert_eq!(snapshot["op"], "snapshot");
    assert_eq!(
        receipt["snapshot_digest"],
        sha256_hex(raw_snapshot.as_bytes())
    );
    assert_eq!(
        receipt["permissions_digest"],
        permissions_digest(&snapshot["capabilities"])
    );
    assert_eq!(receipt["snapshot_cursor"], snapshot["last_seq"]);
    assert!(receipt["catchup_high_water"].as_i64() >= snapshot["last_seq"].as_i64());
    server.stop().await;
}

async fn start(store: SqliteStore) -> RunningServer {
    start_server(store, None, ProviderCatalog::default()).await
}

async fn start_with_frontend(store: SqliteStore, frontend: PathBuf) -> RunningServer {
    start_server(store, Some(frontend), ProviderCatalog::default()).await
}

async fn start_server(
    store: SqliteStore,
    frontend: Option<PathBuf>,
    catalog: ProviderCatalog,
) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind test runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read test runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let mut state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 16),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate test host secret: {error}")),
        ProviderCatalogService::fixed(catalog),
    );
    if let Some(frontend) = frontend {
        state = state.with_frontend(frontend);
    }
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
    let grant = request_ticket(base_url).await;
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

async fn request_ticket(base_url: &str) -> Value {
    let challenge = request_host_challenge(base_url).await;
    let proof = expected_host_request_proof(HOST_TOKEN, &challenge, "general");
    let response = Client::new()
        .post(format!("{base_url}/api/ws-ticket"))
        .header("x-host-challenge", &challenge)
        .header("x-host-meeting", "general")
        .header("x-host-proof", proof)
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
    let ttl_seconds = grant["ttl_seconds"]
        .as_u64()
        .unwrap_or_else(|| panic!("ticket response has no TTL"));
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no proof key"));
    assert_eq!(
        grant["host_response_proof"],
        expected_host_response_proof(HOST_TOKEN, &challenge, ticket, ttl_seconds, proof_key,)
    );
    grant
}

async fn assert_ticket_challenge_is_single_use(base_url: &str) {
    let challenge = request_host_challenge(base_url).await;
    let proof = expected_host_request_proof(HOST_TOKEN, &challenge, "general");
    let client = Client::new();
    for expected in [reqwest::StatusCode::OK, reqwest::StatusCode::UNAUTHORIZED] {
        let response = client
            .post(format!("{base_url}/api/ws-ticket"))
            .header("x-host-challenge", &challenge)
            .header("x-host-meeting", "general")
            .header("x-host-proof", &proof)
            .send()
            .await
            .unwrap_or_else(|error| panic!("exercise single-use host challenge: {error}"));
        assert_eq!(response.status(), expected);
    }
}

async fn request_host_challenge(base_url: &str) -> String {
    let challenge_response = Client::new()
        .get(format!("{base_url}/api/host-challenge"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request host challenge: {error}"));
    assert!(challenge_response.status().is_success());
    let challenge_grant: Value = challenge_response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode host challenge: {error}"));
    let challenge = challenge_grant["challenge"]
        .as_str()
        .unwrap_or_else(|| panic!("host challenge response has no challenge"));
    assert_eq!(
        challenge_grant["host_challenge_proof"],
        expected_hmac(HOST_TOKEN, HOST_CHALLENGE_CONTEXT, &[challenge])
    );
    challenge.to_owned()
}

fn expected_host_request_proof(secret: &str, challenge: &str, meeting_id: &str) -> String {
    expected_hmac(secret, HOST_REQUEST_CONTEXT, &[challenge, meeting_id])
}

fn expected_host_response_proof(
    secret: &str,
    challenge: &str,
    ticket: &str,
    ttl_seconds: u64,
    proof_key: &str,
) -> String {
    expected_hmac(
        secret,
        HOST_RESPONSE_CONTEXT,
        &[challenge, ticket, &ttl_seconds.to_string(), proof_key],
    )
}

fn expected_hmac(secret: &str, context: &str, fields: &[&str]) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .unwrap_or_else(|error| panic!("construct host proof signer: {error}"));
    signer.update(context.as_bytes());
    for field in fields {
        signer.update(field.as_bytes());
        signer.update(&[0]);
    }
    signer
        .finalize()
        .into_bytes()
        .iter()
        .fold(String::with_capacity(64), |mut encoded, byte| {
            write!(encoded, "{byte:02x}")
                .unwrap_or_else(|error| panic!("encode host proof: {error}"));
            encoded
        })
}

async fn subscribe<S>(socket: &mut WebSocketStream<S>, cursor: i64) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let challenge = "d".repeat(64);
    socket
        .send(Message::Text(
            json!({
                "op": "subscribe",
                "streams": ["room_events"],
                "resume_from_seq": cursor,
                "server_challenge": challenge,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send subscription: {error}"));
    let first = receive_json(socket).await;
    if first["op"] == "subscribed" {
        assert_eq!(first["server_challenge"], challenge);
    }
    first
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
    let text = receive_text(socket).await;
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("decode WebSocket JSON: {error}"))
}

async fn receive_text<S>(socket: &mut WebSocketStream<S>) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for WebSocket frame"))
        .unwrap_or_else(|| panic!("WebSocket closed before the expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    String::from_utf8(message.into_data().to_vec())
        .unwrap_or_else(|error| panic!("WebSocket JSON is not UTF-8: {error}"))
}

async fn header_only_request(address: &str, request: &str) -> String {
    let mut socket = TcpStream::connect(address)
        .await
        .unwrap_or_else(|error| panic!("connect raw HTTP client: {error}"));
    socket
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|error| panic!("write raw HTTP headers: {error}"));
    let mut response = Vec::new();
    tokio::time::timeout(Duration::from_secs(1), socket.read_to_end(&mut response))
        .await
        .unwrap_or_else(|_| panic!("server waited for a rejected request body"))
        .unwrap_or_else(|error| panic!("read raw HTTP response: {error}"));
    String::from_utf8(response)
        .unwrap_or_else(|error| panic!("raw HTTP response was not UTF-8: {error}"))
}

async fn bootstrap(store: &SqliteStore) {
    store
        .bootstrap_local_authority("f5d0e901-efbc-491e-8745-781e95cb61f3", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap boundary identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000013",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create boundary room: {error}"));
}
