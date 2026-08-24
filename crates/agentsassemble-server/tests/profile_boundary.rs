use std::{fmt::Write, time::Duration};

use agentsassemble_domain::{Participant, ParticipantStatus, ProviderCatalog, Room, RoomSettings};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use chrono::Utc;
use futures_util::StreamExt;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "profile-boundary-host-token-000000001";
const HOST_REQUEST_CONTEXT: &str = "agentsassemble-host-ticket-request-v1\0";

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
            .unwrap_or_else(|error| panic!("join profile server: {error}"));
    }
}

#[tokio::test]
async fn authenticated_profile_avatar_and_room_projection_survive_restart() {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create profile data root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open profile store: {error}"));
    bootstrap(&store).await;
    let inspection_store = store.clone();
    let server = start(store).await;
    let client = Client::new();
    let avatar_url = assert_profile_auth_and_upload(&client, &server.base_url).await;
    assert_profile_update_and_avatar(&client, &server.base_url, &inspection_store, &avatar_url)
        .await;
    server.stop().await;
    drop(inspection_store);

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen profile store: {error}"));
    let restarted = start(reopened).await;
    let restart_ticket = request_ticket(&restarted.base_url).await;
    let recovered: Value = client
        .get(format!("{}/api/user-profile", restarted.base_url))
        .header("authorization", format!("Bearer {restart_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read restarted profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode restarted profile: {error}"));
    assert_eq!(recovered["profile"]["display_name"], "Canonical Human");
    assert_eq!(recovered["profile"]["avatar_image_url"], avatar_url);
    restarted.stop().await;
}

async fn assert_profile_auth_and_upload(client: &Client, base_url: &str) -> String {
    let unauthorized = client
        .get(format!("{base_url}/api/user-profile"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unauthenticated profile: {error}"));
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    let read_ticket = request_ticket(base_url).await;
    let profile = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {read_ticket}"))
        .header("origin", "tauri://localhost")
        .send()
        .await
        .unwrap_or_else(|error| panic!("read authenticated profile: {error}"));
    assert_eq!(profile.status(), reqwest::StatusCode::OK);
    assert_eq!(
        profile.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let profile: Value = profile
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode profile: {error}"));
    assert_eq!(profile["profile"]["display_name"], "SeiNel");
    assert_eq!(profile["profile"]["revision"], 1);
    let reused = client
        .get(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {read_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse profile ticket: {error}"));
    assert_eq!(reused.status(), reqwest::StatusCode::UNAUTHORIZED);
    let upload_ticket = request_ticket(base_url).await;
    let upload = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .json(&json!({
            "purpose": "profile_avatar", "filename": "../profile.png",
            "content_type": "image/png", "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload profile avatar: {error}"));
    assert_eq!(upload.status(), reqwest::StatusCode::OK);
    let upload: Value = upload
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode profile avatar: {error}"));
    assert_eq!(upload["attachment"]["filename"], "profile.png");
    assert_eq!(upload["attachment"]["content_type"], "image/png");
    let avatar_url = upload["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("profile avatar response has no URL"))
        .to_owned();
    let pending = client
        .get(format!("{base_url}{avatar_url}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read pending profile avatar: {error}"));
    assert_eq!(pending.status(), reqwest::StatusCode::NOT_FOUND);
    let unsupported_ticket = request_ticket(base_url).await;
    let unsupported = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", format!("Bearer {unsupported_ticket}"))
        .json(&json!({
            "purpose": "room_attachment", "filename": "message.png",
            "content_type": "image/png", "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unsupported attachment purpose: {error}"));
    assert_eq!(unsupported.status(), reqwest::StatusCode::BAD_REQUEST);
    avatar_url
}

async fn assert_profile_update_and_avatar(
    client: &Client,
    base_url: &str,
    inspection_store: &SqliteStore,
    avatar_url: &str,
) {
    let mut socket = connect_room(base_url).await;
    assert_eq!(receive_json(&mut socket).await["op"], "snapshot");
    let update_ticket = request_ticket(base_url).await;
    let updated = client
        .post(format!("{base_url}/api/user-profile"))
        .header("authorization", format!("Bearer {update_ticket}"))
        .json(&json!({
            "display_name": "Canonical Human",
            "avatar_image_url": avatar_url,
            "mic_muted": false
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("save profile: {error}"));
    assert_eq!(updated.status(), reqwest::StatusCode::OK);
    let updated: Value = updated
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode saved profile: {error}"));
    assert_eq!(updated["profile"]["revision"], 2);
    let event = receive_json(&mut socket).await;
    assert_eq!(event["op"], "event");
    assert_eq!(event["events"][0]["type"], "participant_updated");
    assert_eq!(event["events"][0]["display_name"], "Canonical Human");
    assert_eq!(event["events"][0]["avatar_image_url"], avatar_url);

    let participant = inspection_store
        .participant("general", "operator-local")
        .await
        .unwrap_or_else(|error| panic!("read projected participant: {error}"));
    assert_eq!(participant.display_name, "Canonical Human");
    assert_eq!(participant.avatar_image_url, avatar_url);
    assert_eq!(participant.role, "host");
    assert_eq!(participant.status, ParticipantStatus::Joined);
    assert!(!participant.muted);

    let avatar = client
        .get(format!("{base_url}{avatar_url}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read profile avatar: {error}"));
    assert_eq!(avatar.status(), reqwest::StatusCode::OK);
    assert_eq!(avatar.headers()["content-type"], "image/png");
    assert_eq!(avatar.headers()["x-content-type-options"], "nosniff");
    assert_eq!(avatar.headers()["cache-control"], "private, no-store");
    assert!(
        avatar.headers()["content-disposition"]
            .to_str()
            .unwrap_or_default()
            .starts_with("inline;")
    );
    assert!(
        avatar
            .bytes()
            .await
            .unwrap_or_default()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );

    socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close profile socket: {error}"));
}

async fn start(store: SqliteStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind profile runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read profile runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 16),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate profile host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    );
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve profile runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        cancellation,
        task,
    }
}

async fn connect_room(
    base_url: &str,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    use futures_util::SinkExt as _;

    let ticket = request_ticket(base_url).await;
    let url = format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    let mut socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect profile socket: {error}"))
        .0;
    socket
        .send(Message::Text(
            json!({"op":"subscribe","streams":["room_events"],"resume_from_seq":0})
                .to_string()
                .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("subscribe profile socket: {error}"));
    socket
}

async fn receive_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = tokio::time::timeout(Duration::from_secs(2), socket.next())
        .await
        .unwrap_or_else(|_| panic!("profile WebSocket frame timed out"))
        .unwrap_or_else(|| panic!("profile WebSocket closed"))
        .unwrap_or_else(|error| panic!("read profile WebSocket: {error}"));
    serde_json::from_slice(&message.into_data())
        .unwrap_or_else(|error| panic!("decode profile WebSocket: {error}"))
}

async fn request_ticket(base_url: &str) -> String {
    let challenge: Value = Client::new()
        .get(format!("{base_url}/api/host-challenge"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request profile host challenge: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode profile host challenge: {error}"));
    let challenge = challenge["challenge"]
        .as_str()
        .unwrap_or_else(|| panic!("profile challenge is missing"));
    let proof = expected_hmac(HOST_REQUEST_CONTEXT, &[challenge, "general"]);
    let grant: Value = Client::new()
        .post(format!("{base_url}/api/ws-ticket"))
        .header("x-host-challenge", challenge)
        .header("x-host-meeting", "general")
        .header("x-host-proof", proof)
        .send()
        .await
        .unwrap_or_else(|error| panic!("request profile ticket: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode profile ticket: {error}"));
    grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("profile ticket is missing"))
        .to_owned()
}

fn expected_hmac(context: &str, fields: &[&str]) -> String {
    let mut signer = Hmac::<Sha256>::new_from_slice(HOST_TOKEN.as_bytes())
        .unwrap_or_else(|error| panic!("construct profile HMAC: {error}"));
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
                .unwrap_or_else(|error| panic!("encode profile HMAC: {error}"));
            encoded
        })
}

async fn bootstrap(store: &SqliteStore) {
    let now = Utc::now();
    let room = Room::new("general".to_owned(), "General".to_owned(), now);
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: "operator-local".to_owned(),
        display_name: "SeiNel".to_owned(),
        avatar_image_url: String::new(),
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
        .unwrap_or_else(|error| panic!("bootstrap profile room: {error}"));
}
