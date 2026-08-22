use std::{collections::BTreeMap, fmt::Write, fs::File, path::Path, time::Duration};

use agentsassemble_domain::{
    Participant, ParticipantStatus, ProviderAvailability, ProviderCatalog, ProviderControl,
    ProviderControlOption, Room, RoomSettings, stable_content_identity,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
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

const HOST_TOKEN: &str = "agent-boundary-host-token-000000001";

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
async fn create_replay_conflict_and_restart_share_one_durable_authority() {
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

async fn start(store: SqliteStore, catalog: ProviderCatalog) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind test runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read test runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 16),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate test host secret: {error}")),
        ProviderCatalogService::fixed(catalog),
    );
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

async fn receive_json<S>(socket: &mut WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
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
    let executable = root.join("provider-fixture");
    std::fs::write(&executable, b"provider fixture")
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
            controls: vec![ProviderControl {
                key: "model".to_owned(),
                label: "Model".to_owned(),
                kind: "combobox".to_owned(),
                options: vec![ProviderControlOption {
                    value: "gpt-5.6-terra".to_owned(),
                    label: "Terra".to_owned(),
                    metadata: BTreeMap::default(),
                }],
                default_value: "gpt-5.6-terra".to_owned(),
            }],
        }],
    }
}
