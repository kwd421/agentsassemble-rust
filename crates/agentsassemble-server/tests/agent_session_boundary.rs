use std::{collections::BTreeMap, path::Path, time::Duration};

#[cfg(unix)]
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use agentsassemble_domain::{
    ProviderAvailability, ProviderCatalog, ProviderControl, ProviderControlOption,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{AppState, TicketStore, issue_local_ticket, serve};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::connect_async;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
#[path = "support/room_portal_fixture.rs"]
mod room_portal_fixture;

#[path = "support/provider_fixture.rs"]
mod provider_fixture;

#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

use room_socket_peer::RoomSocketPeer;

#[cfg(unix)]
#[path = "agent_session_boundary/agent_configuration.rs"]
mod agent_configuration;

#[cfg(unix)]
#[path = "agent_session_boundary/agent_create_start.rs"]
mod agent_create_start;

#[cfg(unix)]
#[path = "agent_session_boundary/agent_interrupt.rs"]
mod agent_interrupt;

#[cfg(unix)]
#[path = "agent_session_boundary/lifecycle_resume_retry.rs"]
mod lifecycle_resume_retry;

static AGENT_BOUNDARY_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct RunningServer {
    base_url: String,
    state: AppState,
    provider_adapter: ProviderAdapter,
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
    let mut socket = connect(&first.base_url, &first.state).await;
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
    socket.close().await;
    first.stop().await;

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen agent store: {error}"));
    let second = start(reopened, catalog).await;
    let mut recovered_socket = connect(&second.base_url, &second.state).await;
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
    let mut socket = connect(&server.base_url, &server.state).await;
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
    let created = receive_command_ack(&mut socket).await;
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
    assert_resident_pause_resume(&mut socket, &lifecycle_payload).await;
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
        "resume-before-server-restart",
        "agent.resume",
        &lifecycle_payload,
    )
    .await;
    let running = receive_until_ack(&mut socket, 3).await;
    assert_eq!(running["result"]["agent_session"]["runtime_status"], "idle");
    assert_session_flag(&running, "provider_session_reused");
    socket.close().await;
    server.stop().await;
    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen lifecycle store: {error}"));
    let restarted = start(reopened, catalog).await;
    let mut recovered_socket = connect(&restarted.base_url, &restarted.state).await;
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
#[allow(clippy::too_many_lines)] // One boundary scenario spans setup, blocked turn, queueing, and publication.
async fn room_turns_publish_provider_finals_without_blocking_room_commands() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create room-turn root: {error}"));
    let transcript = directory.path().join("turn-requests.jsonl");
    let portal_endpoint = directory.path().join("portal-endpoint");
    let portal_token = directory.path().join("portal-token");
    let turn_seen = directory.path().join("turn-seen");
    let release_first = directory.path().join("turn-release-1");
    let release_second = directory.path().join("turn-release-2");
    let fixture = room_portal_fixture::script(
        &transcript,
        &portal_endpoint,
        &portal_token,
        &turn_seen,
        &release_first,
        &release_second,
    );
    let store = SqliteStore::open(&format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("open room-turn store: {error}"));
    bootstrap(&store).await;
    let catalog = agent_catalog_with_fixture(directory.path(), fixture.as_bytes());
    let server = start(store, catalog).await;
    let mut socket = connect(&server.base_url, &server.state).await;
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
    let _started = receive_command_ack(&mut socket).await;

    send_command(
        &mut socket,
        "room-message-1",
        "message.send",
        &json!({"content": "@Terra answer the first room message"}),
    )
    .await;
    let _first_ack = receive_command_ack(&mut socket).await;
    room_portal_fixture::wait_for_turn(&turn_seen, "1").await;
    let endpoint = room_portal_fixture::wait_for_value(&portal_endpoint, "endpoint").await;
    let token = room_portal_fixture::wait_for_value(&portal_token, "token").await;
    send_command(
        &mut socket,
        "room-message-2",
        "message.send",
        &json!({"content": "@Terra queue the second room message"}),
    )
    .await;
    let _second_ack = receive_command_ack(&mut socket).await;
    let first_view = room_portal_fixture::publish(&endpoint, &token, "first room answer").await;
    let second_turn_seen = turn_seen.clone();
    let second_endpoint = endpoint.clone();
    let second_token = token.clone();
    let second_release = release_second.clone();
    let second_portal = tokio::spawn(async move {
        room_portal_fixture::wait_for_turn(&second_turn_seen, "2").await;
        let view =
            room_portal_fixture::publish(&second_endpoint, &second_token, "second room answer")
                .await;
        std::fs::write(&second_release, b"release")
            .unwrap_or_else(|error| panic!("release second room turn fixture: {error}"));
        view
    });
    std::fs::write(&release_first, b"release")
        .unwrap_or_else(|error| panic!("release room turn fixture: {error}"));

    let mut provider_finals = Vec::new();
    for _ in 0..32 {
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
    let second_view = second_portal
        .await
        .unwrap_or_else(|error| panic!("join second portal turn: {error}"));
    let views = format!("{first_view}\n{second_view}");
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
        "runtime_lease_token",
        "runtime_owner_id",
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
        ProviderCatalogService::fixed(catalog),
        provider_adapter.clone(),
    )
    .await
    .unwrap_or_else(|error| panic!("build test app state: {error}"));
    let server_state = state.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .map_err(|error| error.to_string())
    });
    let base_url = format!("http://{address}");
    let client = Client::new();
    let mut ready = false;
    for _ in 0..200 {
        if client
            .get(format!("{base_url}/healthz"))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success())
        {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    if !ready {
        cancellation.cancel();
        let result = task
            .await
            .unwrap_or_else(|error| panic!("failed startup task join: {error}"));
        panic!("server did not admit HTTP after startup recovery: {result:?}");
    }
    RunningServer {
        base_url,
        state: server_state,
        provider_adapter,
        cancellation,
        task,
    }
}

async fn connect(
    base_url: &str,
    state: &AppState,
) -> RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let grant = issue_local_ticket(state, "general")
        .await
        .unwrap_or_else(|error| panic!("issue private-control-equivalent ticket: {error}"));
    let ticket = grant.ticket;
    let socket = connect_async(format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    ))
    .await
    .unwrap_or_else(|error| panic!("connect WebSocket: {error}"))
    .0;
    RoomSocketPeer::new(socket)
}

async fn subscribe<S>(socket: &mut RoomSocketPeer<S>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let receipt = socket.subscribe(0).await;
    assert_eq!(receipt["op"], "subscribed");
    assert_eq!(receipt["streams"], json!(["room_events"]));
}

async fn send_create<S>(socket: &mut RoomSocketPeer<S>, request_id: &str, payload: &Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send_json(&json!({"op": "command", "request_id": request_id, "action": "agent.create", "payload": payload}))
        .await;
}

#[cfg(unix)]
async fn send_command<S>(
    socket: &mut RoomSocketPeer<S>,
    request_id: &str,
    action: &str,
    payload: &Value,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .send_json(&json!({"op": "command", "request_id": request_id, "action": action, "payload": payload}))
        .await;
}

#[cfg(unix)]
async fn assert_resident_pause_resume<S>(socket: &mut RoomSocketPeer<S>, payload: &Value)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_command(socket, "pause-runtime", "agent.pause", payload).await;
    let paused = receive_until_ack(socket, 2).await;
    assert_eq!(
        paused["result"]["agent_session"]["runtime_status"],
        "paused"
    );
    assert_eq!(paused["result"]["process_preserved"], true);
    send_command(socket, "resume-paused-runtime", "agent.resume", payload).await;
    let resumed = receive_until_ack(socket, 2).await;
    assert_eq!(resumed["result"]["agent_session"]["runtime_status"], "idle");
    assert_eq!(resumed["result"]["process_reused"], true);
}

#[cfg(unix)]
async fn receive_until_ack<S>(socket: &mut RoomSocketPeer<S>, limit: usize) -> Value
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

#[cfg(unix)]
async fn receive_command_ack<S>(socket: &mut RoomSocketPeer<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    for _ in 0..32 {
        let frame = receive_json(socket).await;
        if frame["op"] == "ack" {
            return frame;
        }
    }
    panic!("command ACK was not delivered");
}

async fn receive_json<S>(socket: &mut RoomSocketPeer<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket.receive_json().await
}

async fn receive_json_with_timeout<S>(socket: &mut RoomSocketPeer<S>, timeout: Duration) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket.receive_json_with_timeout(timeout).await
}

async fn bootstrap(store: &SqliteStore) {
    store
        .bootstrap_local_authority("91605e65-f5d7-4b58-be4e-962b3041714a", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap boundary identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000014",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create boundary room: {error}"));
}

#[cfg(unix)]
fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

fn agent_catalog(root: &Path) -> ProviderCatalog {
    #[cfg(unix)]
    let fixture: &[u8] = b"#!/bin/sh\nIFS= read -r initialize\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"thread\":{\"id\":\"thread-1\"}}}'\nIFS= read -r forever\n";
    #[cfg(not(unix))]
    let fixture: &[u8] = b"provider fixture";
    agent_catalog_with_fixture(root, fixture)
}

fn agent_catalog_with_fixture(root: &Path, fixture: &[u8]) -> ProviderCatalog {
    let (executable, executable_identity) = provider_fixture::write_codex_bundle(root, fixture);
    ProviderCatalog {
        status: "ready".to_owned(),
        catalog_revision: "catalog-boundary-1".to_owned(),
        discovered_at: "2026-08-22T00:00:00Z".to_owned(),
        providers: vec![ProviderAvailability {
            id: "codex".to_owned(),
            display_name: "Codex".to_owned(),
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            catalog_group: "harness".to_owned(),
            workspace_required: true,
            connection_kind: "native_cli_bridge".to_owned(),
            executable,
            executable_identity,
            default_model: "gpt-5.6-terra".to_owned(),
            interactive: true,
            startable: true,
            available: true,
            discovery_status: "ready".to_owned(),
            catalog_source: "discovered".to_owned(),
            discovery_error_code: String::new(),
            discovery_error: String::new(),
            credential_available: false,
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
    let observed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if path.is_file() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    assert!(
        observed.is_ok(),
        "fixture did not publish {}",
        path.display()
    );
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
