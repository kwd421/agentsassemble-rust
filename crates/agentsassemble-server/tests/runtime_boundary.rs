use std::{path::PathBuf, time::Duration};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    task::JoinHandle,
};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

mod support {
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::{
    local_socket::{connect, request_ticket},
    room_socket_peer::RoomSocketPeer,
};

struct RunningServer {
    base_url: String,
    state: AppState,
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
    let retired_ticket_route = Client::new()
        .post(format!("{}/api/ws-ticket", first_server.base_url))
        .json(&json!({"meeting_id": "general"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request retired ticket route: {error}"));
    assert_eq!(
        retired_ticket_route.status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let retired_challenge_route = Client::new()
        .get(format!("{}/api/host-challenge", first_server.base_url))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request retired challenge route: {error}"));
    assert_eq!(
        retired_challenge_route.status(),
        reqwest::StatusCode::NOT_FOUND
    );
    let mut first_socket = connect(&first_server.base_url, &first_server.state, "general").await;
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
    assert_eq!(ack["resolution"], "committed");
    assert_eq!(event["events"][0]["seq"], 2);
    assert_eq!(ack["result"]["event_seq"], 2);
    assert_eq!(ack["result"]["event"]["id"], event["events"][0]["id"]);
    assert_eq!(event["events"][0]["id"].as_str().map(str::len), Some(36));
    first_socket.close().await;
    first_server.stop().await;

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen store: {error}"));
    let second_server = start(reopened).await;
    let mut second_socket = connect(&second_server.base_url, &second_server.state, "general").await;
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
    assert_eq!(retry["resolution"], "committed");
    assert_eq!(retry["deduplicated"], true);
    assert_eq!(retry["result"]["event_seq"], 2);
    let mut cursor_ahead = connect(&second_server.base_url, &second_server.state, "general").await;
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
async fn websocket_connection_limit_is_shared_by_one_principal() {
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
    let mut sockets = Vec::new();
    for _ in 0..8 {
        sockets.push(connect(&server.base_url, &server.state, "general").await);
    }

    let grant = request_ticket(&server.state, "general").await;
    let ticket = grant.ticket;
    let url = format!(
        "{}/ws?ticket={ticket}",
        server.base_url.replacen("http://", "ws://", 1)
    );
    let Err(error) = connect_async(url).await else {
        panic!("the ninth connection for one principal was admitted");
    };
    let tokio_tungstenite::tungstenite::Error::Http(response) = error else {
        panic!("unexpected ninth-connection failure: {error}");
    };
    assert_eq!(response.status(), reqwest::StatusCode::SERVICE_UNAVAILABLE);

    for socket in &mut sockets {
        socket.close().await;
    }
    server.stop().await;
}

#[tokio::test]
async fn retired_http_ticket_routes_are_absent_at_the_tcp_boundary() {
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

    let retired_ticket = header_only_request(
        &address,
        &format!(
            "POST /api/ws-ticket HTTP/1.1\r\nHost: {address}\r\nContent-Length: 1048576\r\n\r\n"
        ),
    )
    .await;
    assert!(retired_ticket.starts_with("HTTP/1.1 404"));

    let retired_challenge = header_only_request(
        &address,
        &format!("GET /api/host-challenge HTTP/1.1\r\nHost: {address}\r\n\r\n"),
    )
    .await;
    assert!(retired_challenge.starts_with("HTTP/1.1 404"));
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
    let mut socket = connect(&server.base_url, &server.state, "general").await;
    subscribe(&mut socket, 0).await;
    let _ = receive_json(&mut socket).await;
    let content = "x".repeat(12_000);
    for index in 0..32 {
        socket
            .send_json(&json!({
                "op": "command",
                "request_id": format!("snapshot-budget-{index}"),
                "action": "message.send",
                "payload": {"content": content}
            }))
            .await;
        for _ in 0..2 {
            let _ = receive_json(&mut socket).await;
        }
    }
    socket.close().await;

    let mut resumed = connect(&server.base_url, &server.state, "general").await;
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
async fn static_frontend_has_browser_security_and_cache_headers() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
    let frontend = directory.path().join("frontend");
    tokio::fs::create_dir_all(frontend.join("assets"))
        .await
        .unwrap_or_else(|error| panic!("create frontend root: {error}"));
    tokio::fs::write(
        frontend.join("index.html"),
        "<!doctype html><title>test</title><script src=\"./assets/app.js\"></script>",
    )
    .await
    .unwrap_or_else(|error| panic!("write frontend index: {error}"));
    tokio::fs::write(frontend.join("assets/app.js"), "globalThis.loaded = true;")
        .await
        .unwrap_or_else(|error| panic!("write frontend asset: {error}"));
    let database_url = format!("sqlite://{}/runtime.sqlite3", directory.path().display());
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
    assert_static_frontend_headers(&response);
    for entrance in ["/join?token=one-use", "/join/", "/pair", "/pair/"] {
        let response = Client::new()
            .get(format!("{}{entrance}", server.base_url))
            .send()
            .await
            .unwrap_or_else(|error| panic!("request browser entrance {entrance}: {error}"));
        assert!(
            response.status().is_success(),
            "browser entrance {entrance}"
        );
        assert_static_frontend_headers(&response);
        let asset_url = response
            .url()
            .join("./assets/app.js")
            .unwrap_or_else(|error| panic!("resolve browser asset from {entrance}: {error}"));
        let body = response
            .text()
            .await
            .unwrap_or_else(|error| panic!("read browser entrance {entrance}: {error}"));
        assert!(body.contains("./assets/app.js"));
        let asset = Client::new()
            .get(asset_url)
            .send()
            .await
            .unwrap_or_else(|error| panic!("request browser asset from {entrance}: {error}"));
        assert!(asset.status().is_success(), "browser asset from {entrance}");
        assert_static_frontend_headers(&asset);
    }
    server.stop().await;
}

fn assert_static_frontend_headers(response: &reqwest::Response) {
    assert!(response.headers().contains_key("content-security-policy"));
    for (name, expected) in [
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("connection", "close"),
        ("cache-control", "no-cache"),
    ] {
        assert_eq!(
            response
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok()),
            Some(expected)
        );
    }
}

#[tokio::test]
async fn binary_frame_is_rejected_and_closed() {
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
    let mut socket = connect(&server.base_url, &server.state, "general").await;
    subscribe(&mut socket, 0).await;
    let snapshot = receive_json(&mut socket).await;
    assert_eq!(snapshot["op"], "snapshot");
    socket.send_binary(vec![1, 2, 3]).await;
    let nack = receive_json(&mut socket).await;
    assert_eq!(nack["resolution"], "unresolved");
    assert_eq!(nack["error"]["code"], "binary_frame_unsupported");
    assert!(socket.wait_closed().await);
    server.stop().await;
}

#[tokio::test]
async fn structurally_valid_retired_authenticated_envelope_has_no_compatibility_decoder() {
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
    let mut socket = connect(&server.base_url, &server.state, "general").await;
    subscribe(&mut socket, 0).await;
    assert_eq!(receive_json(&mut socket).await["op"], "snapshot");
    let retired_inner_frame = json!({
        "op": "command",
        "request_id": "retired-envelope",
        "action": "message.send",
        "payload": {"content": "attacker content"}
    });
    socket
        .send_json(&json!({
            "op": "authenticated",
            "counter": 1,
            "payload": STANDARD.encode(retired_inner_frame.to_string()),
            "proof": "0".repeat(64),
        }))
        .await;
    let nack = receive_json(&mut socket).await;
    assert_eq!(nack["resolution"], "unresolved");
    assert_eq!(nack["error"]["code"], "frame_schema_invalid");
    assert!(socket.wait_closed().await);
    server.stop().await;

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen tamper test store: {error}"));
    let snapshot = reopened
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("read tamper test snapshot: {error}"));
    assert!(
        snapshot
            .events
            .iter()
            .all(|event| event.content.as_deref() != Some("attacker content"))
    );
}

#[tokio::test]
async fn websocket_snapshot_is_bound_to_the_private_ticket_scope_and_finite_cursor() {
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
    let grant = request_ticket(&server.state, "general").await;
    let ticket = grant.ticket;
    let url = format!(
        "{}/ws?ticket={ticket}",
        server.base_url.replacen("http://", "ws://", 1)
    );
    let mut socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect bounded WebSocket: {error}"))
        .0;
    socket
        .send(Message::Text(
            json!({
                "op": "subscribe",
                "streams": ["room_events"],
                "resume_from_seq": 0,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send bounded subscription: {error}"));
    let receipt = receive_raw_json(&mut socket).await;
    assert_eq!(receipt["op"], "subscribed");
    let raw_snapshot = receive_text(&mut socket).await;
    let snapshot: Value = serde_json::from_str(&raw_snapshot)
        .unwrap_or_else(|error| panic!("decode bounded snapshot: {error}"));
    assert_eq!(snapshot["op"], "snapshot");
    assert_eq!(receipt["room_id"], "general");
    assert_eq!(receipt["participant_id"], "operator-local");
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
        ProviderCatalogService::fixed(catalog),
    )
    .await
    .unwrap_or_else(|error| panic!("build test app state: {error}"));
    if let Some(frontend) = frontend {
        state = state.with_frontend(frontend);
    }
    let server_state = state.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve test runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        state: server_state,
        cancellation,
        task,
    }
}

async fn subscribe<S>(socket: &mut RoomSocketPeer<S>, cursor: i64) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket.subscribe(cursor).await
}

async fn send_command<S>(socket: &mut RoomSocketPeer<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "boundary-request-1",
            "action": "message.send",
            "payload": {"content": "boundary hello"}
        }))
        .await;
}

async fn receive_json<S>(socket: &mut RoomSocketPeer<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket.receive_json().await
}

async fn receive_raw_json<S>(socket: &mut WebSocketStream<S>) -> Value
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
