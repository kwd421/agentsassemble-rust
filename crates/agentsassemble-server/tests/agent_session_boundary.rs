use std::{collections::BTreeMap, fmt::Write, fs::File, path::Path, time::Duration};

use agentsassemble_domain::{
    Participant, ParticipantStatus, ProviderAvailability, ProviderCatalog, ProviderControl,
    ProviderControlOption, Room, RoomSettings, stable_content_identity,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use chrono::Utc;
use futures_util::{SinkExt, StreamExt};
use hmac::{Hmac, Mac};
use reqwest::Client;
use same_file::Handle;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{WebSocketStream, connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
#[path = "support/room_portal_fixture.rs"]
mod room_portal_fixture;

const HOST_TOKEN: &str = "agent-boundary-host-token-000000001";
static AGENT_BOUNDARY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct RunningServer {
    base_url: String,
    cancellation: CancellationToken,
    task: JoinHandle<Result<(), String>>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("server task join: {error}"))
            .unwrap_or_else(|error| panic!("stop test runtime: {error}"));
    }

    #[cfg(unix)]
    async fn stop_with_interrupted_command(self) {
        self.cancellation.cancel();
        let result = self
            .task
            .await
            .unwrap_or_else(|error| panic!("interrupted server task join: {error}"));
        assert!(
            result.is_err(),
            "an interrupted room command must remain visible during shutdown"
        );
    }
}

#[tokio::test]
async fn create_replay_conflict_and_restart_share_one_durable_authority() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create agent data root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open agent store: {error}"));
    bootstrap(&store).await;
    let catalog = agent_catalog(directory.path());
    let first = start(store, catalog.clone()).await;
    let mut socket = connect(&first.base_url).await;
    subscribe(&mut socket).await;
    let snapshot = receive_json(&mut socket).await;
    assert_eq!(
        snapshot["provider_catalog"]["catalog_revision"],
        "catalog-boundary-1"
    );
    let payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "start_now": false,
    });
    send_create(&mut socket, "agent-create-1", &payload).await;
    let frames = [
        receive_json(&mut socket).await,
        receive_json(&mut socket).await,
    ];
    let ack = frames
        .iter()
        .find(|frame| frame["op"] == "ack")
        .unwrap_or_else(|| panic!("agent create ACK was not delivered"));
    let session_id = ack["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("agent create ACK has no session id"))
        .to_owned();
    assert_public_session(&ack["result"]["agent_session"]);
    assert!(frames.iter().any(|frame| frame["op"] == "event"));
    send_create(&mut socket, "agent-create-1", &payload).await;
    let replay = receive_json(&mut socket).await;
    assert_eq!(replay["op"], "ack");
    assert_eq!(replay["deduplicated"], true);
    let mut changed_payload = payload.clone();
    changed_payload["display_name"] = json!("Changed");
    send_create(&mut socket, "agent-create-1", &changed_payload).await;
    assert_eq!(
        receive_json(&mut socket).await["error"]["code"],
        "command_conflict"
    );
    socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close agent socket: {error}"));
    first.stop().await;

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen agent store: {error}"));
    let second = start(reopened, catalog).await;
    let mut recovered_socket = connect(&second.base_url).await;
    subscribe(&mut recovered_socket).await;
    let recovered = receive_json(&mut recovered_socket).await;
    assert_eq!(recovered["agent_sessions"][0]["session_id"], session_id);
    assert_eq!(recovered["agent_sessions"][0]["runtime_status"], "stopped");
    assert_eq!(recovered["agent_sessions"][0]["model"], "gpt-5.6-terra");
    assert_public_session(&recovered["agent_sessions"][0]);
    second.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn lifecycle_commands_use_the_owned_codex_app_server_before_committing() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create lifecycle root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open lifecycle store: {error}"));
    bootstrap(&store).await;
    let catalog = agent_catalog(directory.path());
    let server = start(store, catalog.clone()).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let create_payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": false,
    });
    send_create(&mut socket, "create-for-lifecycle", &create_payload).await;
    let created = receive_until_ack(&mut socket, 2).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"));
    let lifecycle_payload = json!({"agent_id": session_id});
    send_command(
        &mut socket,
        "first-start",
        "agent.start",
        &lifecycle_payload,
    )
    .await;
    let started = receive_until_ack(&mut socket, 4).await;
    assert_eq!(started["result"]["agent_session"]["runtime_status"], "idle");
    assert_eq!(started["result"]["runtime_reused"], false);
    assert_session_flag(&started, "provider_session_active");
    send_command(
        &mut socket,
        "second-start",
        "agent.start",
        &lifecycle_payload,
    )
    .await;
    let reused = receive_until_ack(&mut socket, 3).await;
    assert_eq!(reused["result"]["runtime_reused"], true);
    assert_session_flag(&reused, "provider_session_reused");
    send_command(
        &mut socket,
        "stop-runtime",
        "agent.stop",
        &lifecycle_payload,
    )
    .await;
    let stopped = receive_until_ack(&mut socket, 3).await;
    assert_eq!(
        stopped["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    send_command(
        &mut socket,
        "start-before-server-restart",
        "agent.start",
        &lifecycle_payload,
    )
    .await;
    let running = receive_until_ack(&mut socket, 3).await;
    assert_eq!(running["result"]["agent_session"]["runtime_status"], "idle");
    assert_session_flag(&running, "provider_session_reused");
    socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close lifecycle socket: {error}"));
    server.stop().await;
    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen lifecycle store: {error}"));
    let restarted = start(reopened, catalog).await;
    let mut recovered_socket = connect(&restarted.base_url).await;
    subscribe(&mut recovered_socket).await;
    let recovered = receive_json(&mut recovered_socket).await;
    assert_eq!(
        recovered["agent_sessions"][0]["runtime_status"],
        "disconnected"
    );
    assert_eq!(
        recovered["agent_sessions"][0]["last_error_code"],
        "server_restarted"
    );
    restarted.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn shutdown_checkpoints_gone_after_aborting_initialization() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create cancellation root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open cancellation store: {error}"));
    bootstrap(&store).await;
    let started_path = directory.path().join("initialization-started");
    let release_path = directory.path().join("release-initialization");
    let fixture = format!(
        "#!/bin/sh\nprintf '%s' \"$$\" > {}\nIFS= read -r initialize\nwhile [ ! -f {} ]; do :; done\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r forever\n",
        shell_quote(&started_path),
        shell_quote(&release_path),
    );
    let catalog = agent_catalog_with_fixture(directory.path(), fixture.as_bytes());
    let server = start(store, catalog.clone()).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let create_payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": false,
    });
    send_create(&mut socket, "create-cancelled-start", &create_payload).await;
    let created = receive_until_ack(&mut socket, 2).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("cancelled fixture session has no id"))
        .to_owned();
    let lifecycle_payload = json!({"agent_id": session_id});
    send_command(
        &mut socket,
        "cancelled-start",
        "agent.start",
        &lifecycle_payload,
    )
    .await;
    wait_for_file(&started_path).await;
    server.stop_with_interrupted_command().await;
    std::fs::write(&release_path, b"release")
        .unwrap_or_else(|error| panic!("release initialization fixture: {error}"));

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen cancellation store: {error}"));
    let restarted = start(reopened, catalog).await;
    let mut recovered_socket = connect(&restarted.base_url).await;
    subscribe(&mut recovered_socket).await;
    let recovered = receive_json(&mut recovered_socket).await;
    assert_eq!(recovered["agent_sessions"][0]["runtime_status"], "starting");
    send_command(
        &mut recovered_socket,
        "cancelled-start",
        "agent.start",
        &lifecycle_payload,
    )
    .await;
    let resumed = receive_until_ack(&mut recovered_socket, 3).await;
    assert_eq!(resumed["result"]["agent_session"]["runtime_status"], "idle");
    restarted.stop().await;
}

#[cfg(unix)]
#[tokio::test]
#[allow(clippy::too_many_lines)] // One boundary scenario spans setup, blocked turn, queueing, and publication.
async fn room_turns_publish_provider_finals_without_blocking_room_commands() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create room-turn root: {error}"));
    let transcript = directory.path().join("turn-requests.jsonl");
    let observed_views = directory.path().join("observed-room-views.txt");
    let turn_seen = directory.path().join("turn-seen");
    let release = directory.path().join("turn-release");
    let fixture = room_portal_fixture::script(&transcript, &observed_views, &turn_seen, &release);
    let store = SqliteStore::open(&format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("open room-turn store: {error}"));
    bootstrap(&store).await;
    let catalog = agent_catalog_with_fixture(directory.path(), fixture.as_bytes());
    let server = start(store, catalog).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    send_create(
        &mut socket,
        "create-room-turn-agent",
        &json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-boundary-1",
            "display_name": "Terra",
            "workspace": directory.path(),
            "model": "gpt-5.6-terra",
            "permission_mode": "meeting_read_only",
            "start_now": false,
        }),
    )
    .await;
    let created = receive_until_ack(&mut socket, 2).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created room-turn session has no id"));
    send_command(
        &mut socket,
        "start-room-turn-agent",
        "agent.start",
        &json!({"agent_id": session_id}),
    )
    .await;
    let _started = receive_until_ack(&mut socket, 4).await;

    send_command(
        &mut socket,
        "room-message-1",
        "message.send",
        &json!({"content": "@Terra answer the first room message"}),
    )
    .await;
    let _first_ack = receive_until_ack(&mut socket, 5).await;
    wait_for_file(&turn_seen).await;
    send_command(
        &mut socket,
        "room-message-2",
        "message.send",
        &json!({"content": "@Terra queue the second room message"}),
    )
    .await;
    let _second_ack = receive_until_ack(&mut socket, 2).await;
    std::fs::write(&release, b"release")
        .unwrap_or_else(|error| panic!("release room turn fixture: {error}"));

    let mut provider_finals = Vec::new();
    for _ in 0..9 {
        let frame = receive_json_with_timeout(&mut socket, Duration::from_secs(5)).await;
        for event in frame["events"].as_array().into_iter().flatten() {
            if event["type"] == "message_final" && event["actor"]["participant_type"] == "agent" {
                provider_finals.push(event["content"].as_str().unwrap_or_default().to_owned());
            }
        }
        if provider_finals.len() == 2 {
            break;
        }
    }
    assert_eq!(provider_finals, ["first room answer", "second room answer"]);
    let views = std::fs::read_to_string(&observed_views)
        .unwrap_or_else(|error| panic!("read observed RoomPortal views: {error}"));
    assert!(views.contains("answer the first room message"));
    assert!(views.contains("queue the second room message"));
    let requests = std::fs::read_to_string(&transcript)
        .unwrap_or_else(|error| panic!("read room-turn transcript: {error}"))
        .lines()
        .map(|line| {
            serde_json::from_str::<Value>(line)
                .unwrap_or_else(|error| panic!("decode room-turn request: {error}"))
        })
        .collect::<Vec<_>>();
    let turns = requests
        .iter()
        .filter(|request| request["method"] == "turn/start")
        .collect::<Vec<_>>();
    assert_eq!(turns.len(), 2);
    assert!(
        turns[0]["params"]["input"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("read_discussion")
    );
    assert!(
        turns[1]["params"]["input"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .contains("read_discussion")
    );
    server.stop().await;
}

fn assert_public_session(session: &Value) {
    for private in [
        "workspace",
        "workspace_identity",
        "executable",
        "executable_identity",
        "runtime_profile_key",
        "runtime_profile_version",
        "provider_session_id",
        "runtime_handle_id",
        "lifecycle_intent_action",
        "lifecycle_intent_id",
        "lifecycle_intent_status",
    ] {
        assert!(
            session.get(private).is_none(),
            "public Agent Session exposed {private}"
        );
    }
}

#[cfg(unix)]
fn assert_session_flag(response: &Value, field: &str) {
    assert_eq!(response["result"]["agent_session"][field], true);
}

async fn start(store: SqliteStore, catalog: ProviderCatalog) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind test runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read test runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    #[cfg(unix)]
    let provider_adapter = ProviderAdapter::with_guardian_executable(std::path::Path::new(env!(
        "CARGO_BIN_EXE_agentsassemble-server"
    )));
    #[cfg(not(unix))]
    let provider_adapter = ProviderAdapter::new();
    let state = AppState::local_with_provider_adapter(
        store,
        TicketStore::new(Duration::from_secs(30), 16),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate test host secret: {error}")),
        ProviderCatalogService::fixed(catalog),
        provider_adapter,
    );
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .map_err(|error| error.to_string())
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
    let challenge = request_challenge(base_url).await;
    let proof = expected_hmac(
        "agentsassemble-host-ticket-request-v1\0",
        &[&challenge, "general"],
    );
    let grant: Value = Client::new()
        .post(format!("{base_url}/api/ws-ticket"))
        .header("x-host-challenge", challenge)
        .header("x-host-meeting", "general")
        .header("x-host-proof", proof)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request ticket: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode ticket: {error}"));
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket response has no ticket"));
    connect_async(format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    ))
    .await
    .unwrap_or_else(|error| panic!("connect WebSocket: {error}"))
    .0
}

async fn request_challenge(base_url: &str) -> String {
    let grant: Value = Client::new()
        .get(format!("{base_url}/api/host-challenge"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request host challenge: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode host challenge: {error}"));
    grant["challenge"]
        .as_str()
        .unwrap_or_else(|| panic!("challenge grant has no challenge"))
        .to_owned()
}

fn expected_hmac(context: &str, fields: &[&str]) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(HOST_TOKEN.as_bytes())
        .unwrap_or_else(|error| panic!("construct host signer: {error}"));
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
                .unwrap_or_else(|error| panic!("encode proof byte: {error}"));
            encoded
        })
}

async fn subscribe<S>(socket: &mut WebSocketStream<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({"op": "subscribe", "streams": ["room_events"], "resume_from_seq": 0})
                .to_string()
                .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send subscription: {error}"));
}

async fn send_create<S>(socket: &mut WebSocketStream<S>, request_id: &str, payload: &Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({"op": "command", "request_id": request_id, "action": "agent.create", "payload": payload})
                .to_string()
                .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send agent create: {error}"));
}

#[cfg(unix)]
async fn send_command<S>(
    socket: &mut WebSocketStream<S>,
    request_id: &str,
    action: &str,
    payload: &Value,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send(Message::Text(
            json!({"op": "command", "request_id": request_id, "action": action, "payload": payload})
                .to_string()
                .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("send {action}: {error}"));
}

#[cfg(unix)]
async fn receive_until_ack<S>(socket: &mut WebSocketStream<S>, limit: usize) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let mut ack = None;
    for _ in 0..limit {
        let frame = receive_json(socket).await;
        if frame["op"] == "ack" {
            ack = Some(frame);
        }
    }
    ack.unwrap_or_else(|| panic!("command ACK was not delivered within {limit} frames"))
}

async fn receive_json<S>(socket: &mut WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    receive_json_with_timeout(socket, Duration::from_secs(2)).await
}

async fn receive_json_with_timeout<S>(socket: &mut WebSocketStream<S>, timeout: Duration) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(timeout, socket.next())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for WebSocket frame"))
        .unwrap_or_else(|| panic!("WebSocket closed before expected frame"))
        .unwrap_or_else(|error| panic!("receive WebSocket frame: {error}"));
    serde_json::from_slice(&message.into_data())
        .unwrap_or_else(|error| panic!("decode WebSocket JSON: {error}"))
}

async fn bootstrap(store: &SqliteStore) {
    let now = Utc::now();
    let room = Room::new("general".to_owned(), "General".to_owned(), now);
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: "operator-local".to_owned(),
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
        .initialize_room(
            &room,
            &RoomSettings::defaults("General".to_owned()),
            &participant,
        )
        .await
        .unwrap_or_else(|error| panic!("bootstrap boundary room: {error}"));
}

fn agent_catalog(root: &Path) -> ProviderCatalog {
    #[cfg(unix)]
    let fixture: &[u8] = b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\nIFS= read -r forever\n";
    #[cfg(not(unix))]
    let fixture: &[u8] = b"provider fixture";
    agent_catalog_with_fixture(root, fixture)
}

fn agent_catalog_with_fixture(root: &Path, fixture: &[u8]) -> ProviderCatalog {
    let executable = root.join("provider-fixture");
    std::fs::write(&executable, fixture)
        .unwrap_or_else(|error| panic!("write test executable: {error}"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut permissions = std::fs::metadata(&executable)
            .unwrap_or_else(|error| panic!("read test executable mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions)
            .unwrap_or_else(|error| panic!("set test executable mode: {error}"));
    }
    let executable = executable
        .canonicalize()
        .unwrap_or_else(|error| panic!("resolve test executable: {error}"));
    let mut executable_file =
        File::open(&executable).unwrap_or_else(|error| panic!("open test executable: {error}"));
    let executable_handle = Handle::from_file(
        executable_file
            .try_clone()
            .unwrap_or_else(|error| panic!("clone test executable: {error}")),
    )
    .unwrap_or_else(|error| panic!("identify test executable: {error}"));
    let executable_identity = stable_content_identity(&executable_handle, &mut executable_file)
        .unwrap_or_else(|error| panic!("hash test executable: {error}"));
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "catalog-boundary-1".to_owned(),
        discovered_at: "2026-08-22T00:00:00Z".to_owned(),
        providers: vec![ProviderAvailability {
            id: "codex".to_owned(),
            display_name: "Codex".to_owned(),
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            catalog_group: "subscription".to_owned(),
            workspace_required: true,
            connection_kind: "native_cli_bridge".to_owned(),
            executable: executable.to_string_lossy().into_owned(),
            executable_identity,
            default_model: "gpt-5.6-terra".to_owned(),
            interactive: true,
            startable: true,
            available: true,
            discovery_status: "ready".to_owned(),
            catalog_source: "discovered".to_owned(),
            discovery_error_code: String::new(),
            discovery_error: String::new(),
            login_available: true,
            login_label: "Login".to_owned(),
            login_flow: "browser_oauth".to_owned(),
            controls: vec![
                ProviderControl {
                    key: "model".to_owned(),
                    label: "Model".to_owned(),
                    kind: "combobox".to_owned(),
                    options: vec![ProviderControlOption {
                        value: "gpt-5.6-terra".to_owned(),
                        label: "Terra".to_owned(),
                        metadata: BTreeMap::default(),
                    }],
                    default_value: "gpt-5.6-terra".to_owned(),
                },
                ProviderControl {
                    key: "permission_mode".to_owned(),
                    label: "Permission".to_owned(),
                    kind: "select".to_owned(),
                    options: vec![ProviderControlOption {
                        value: "meeting_read_only".to_owned(),
                        label: "Read only".to_owned(),
                        metadata: BTreeMap::default(),
                    }],
                    default_value: "meeting_read_only".to_owned(),
                },
            ],
        }],
    }
}

#[cfg(unix)]
async fn wait_for_file(path: &Path) {
    for _ in 0..200 {
        if path.is_file() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fixture did not publish {}", path.display());
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
