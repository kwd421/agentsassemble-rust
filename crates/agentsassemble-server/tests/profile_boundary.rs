use std::{fmt::Write, time::Duration};

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ParticipantRole, ParticipantStatus, ProviderCatalog, RoomSettings,
    public_settings,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::Sha256;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::connect_async;
use tokio_util::sync::CancellationToken;

#[path = "support/subscription_proof.rs"]
mod subscription_proof;

use subscription_proof::AuthenticatedTestSocket;

const HOST_TOKEN: &str = "profile-boundary-host-token-000000001";
const HOST_REQUEST_CONTEXT: &str = "agentsassemble-host-ticket-request-v1\0";

struct RunningServer {
    base_url: String,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

#[tokio::test]
async fn server_operator_profile_authority_works_before_the_first_room() {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open zero-room profile store: {error}"));
    store
        .bootstrap_local_authority("4f6f1746-fc26-4a10-a7a0-8651a17baa43", "Zero Room Operator")
        .await
        .unwrap_or_else(|error| panic!("bootstrap zero-room operator: {error}"));
    let inspection_store = store.clone();
    let tickets = TicketStore::new(Duration::from_secs(30), 16);
    let issuer = tickets.clone();
    let server = start_with_tickets(store, tickets).await;
    let client = Client::new();

    let read_ticket = issue_operator_ticket(&issuer).await;
    let profile: Value = client
        .get(format!("{}/api/user-profile", server.base_url))
        .header("authorization", format!("Bearer {read_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read zero-room profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode zero-room profile: {error}"));
    assert_eq!(profile["profile"]["display_name"], "Zero Room Operator");

    let upload_ticket = issue_operator_ticket(&issuer).await;
    let upload: Value = client
        .post(format!("{}/api/attachments", server.base_url))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .json(&json!({
            "purpose": "profile_avatar", "filename": "profile.png",
            "content_type": "image/png", "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload zero-room avatar: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode zero-room avatar: {error}"));
    let avatar_url = upload["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("zero-room avatar URL is missing"));

    let update_ticket = issue_operator_ticket(&issuer).await;
    let updated = client
        .post(format!("{}/api/user-profile", server.base_url))
        .header("authorization", format!("Bearer {update_ticket}"))
        .json(&json!({
            "expected_revision": 1,
            "display_name": "Canonical Before Room",
            "avatar_image_url": avatar_url
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("update zero-room profile: {error}"));
    assert_eq!(updated.status(), reqwest::StatusCode::OK);
    assert!(
        inspection_store
            .list_room_directory(true)
            .await
            .unwrap_or_else(|error| panic!("inspect zero-room directory: {error}"))
            .is_empty()
    );
    let stored = inspection_store
        .local_operator_profile()
        .await
        .unwrap_or_else(|error| panic!("inspect zero-room profile: {error}"));
    assert_eq!(stored.display_name, "Canonical Before Room");
    assert_eq!(stored.avatar_image_url, avatar_url);

    let stale_ticket = issue_operator_ticket(&issuer).await;
    let stale = client
        .post(format!("{}/api/user-profile", server.base_url))
        .header("authorization", format!("Bearer {stale_ticket}"))
        .json(&json!({
            "expected_revision": 1,
            "display_name": "Stale Overwrite"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject stale zero-room profile: {error}"));
    assert_eq!(stale.status(), reqwest::StatusCode::CONFLICT);
    let stale: Value = stale
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode stale profile rejection: {error}"));
    assert_eq!(stale["code"], "profile_revision_conflict");
    assert_eq!(
        inspection_store
            .local_operator_profile()
            .await
            .unwrap_or_else(|error| panic!("inspect profile after stale write: {error}"))
            .display_name,
        "Canonical Before Room"
    );
    server.stop().await;
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

#[tokio::test]
async fn room_appearance_upload_preview_bind_and_member_read_use_exact_tickets() {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open appearance HTTP store: {error}"));
    bootstrap(&store).await;
    let inspection_store = store.clone();
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let issuer = tickets.clone();
    let server = start_with_tickets(store, tickets).await;
    let client = Client::new();

    let upload_ticket = issue_appearance_upload(&issuer).await;
    let upload = client
        .post(format!("{}/api/attachments", server.base_url))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .json(&json!({
            "purpose": "room_appearance", "filename": "../banner.webp",
            "content_type": "image/png", "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload room appearance: {error}"));
    assert_eq!(upload.status(), reqwest::StatusCode::OK);
    let upload: Value = upload
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode room appearance upload: {error}"));
    let asset_id = upload["attachment"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("appearance asset ID is missing"));
    let asset_url = upload["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("appearance asset URL is missing"));
    assert!(asset_id.starts_with("ra_"));
    assert_eq!(asset_url, format!("/api/attachments/{asset_id}?view=1"));

    let preview_ticket = issuer
        .issue_pending_preview_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            asset_id.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue pending appearance preview: {error}"))
        .ticket;
    let preview = client
        .get(format!("{}{asset_url}", server.base_url))
        .header("authorization", format!("Bearer {preview_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("preview pending appearance: {error}"));
    assert_static_private_png(preview).await;

    let principal = local_principal();
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("appearance HTTP revision: {error}"))
        .settings_revision;
    inspection_store
        .execute_room_settings_update(
            &principal,
            "appearance-http-bind",
            &json!({
                "expected_revision": revision,
                "appearance": {"banner_image_url": asset_url, "banner_preset": "custom"}
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("bind HTTP appearance: {error}"));

    let exact_query_ticket = issue_bound_read(&issuer, asset_id).await;
    let wrong_query = client
        .get(format!(
            "{}/api/attachments/{asset_id}?download=1",
            server.base_url
        ))
        .header("authorization", format!("Bearer {exact_query_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject appearance download query: {error}"));
    assert_eq!(wrong_query.status(), reqwest::StatusCode::BAD_REQUEST);
    let bound = client
        .get(format!("{}{asset_url}", server.base_url))
        .header("authorization", format!("Bearer {exact_query_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read bound appearance: {error}"));
    assert_static_private_png(bound).await;

    let unauthorized = client
        .get(format!("{}{asset_url}", server.base_url))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject public room appearance: {error}"));
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);
    server.stop().await;
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
            "expected_revision": 1,
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
    assert_eq!(participant.role, ParticipantRole::Human);
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

    socket.close().await;
}

async fn start(store: SqliteStore) -> RunningServer {
    start_with_tickets(store, TicketStore::new(Duration::from_secs(30), 16)).await
}

async fn start_with_tickets(store: SqliteStore, tickets: TicketStore) -> RunningServer {
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
        tickets,
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate profile host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build profile app state: {error}"));
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

async fn issue_operator_ticket(tickets: &TicketStore) -> String {
    tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue server operator profile ticket: {error}"))
        .ticket
}

async fn issue_appearance_upload(tickets: &TicketStore) -> String {
    tickets
        .issue_appearance_upload(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue appearance upload: {error}"))
        .ticket
}

async fn issue_bound_read(tickets: &TicketStore, asset_id: &str) -> String {
    tickets
        .issue_bound_appearance_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            asset_id.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue bound appearance read: {error}"))
        .ticket
}

fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "SeiNel".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

async fn assert_static_private_png(response: reqwest::Response) {
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["referrer-policy"], "no-referrer");
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap_or_default()
            .starts_with("inline;")
    );
    assert!(
        response
            .bytes()
            .await
            .unwrap_or_default()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

async fn connect_room(
    base_url: &str,
) -> AuthenticatedTestSocket<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let grant = request_ticket_grant(base_url).await;
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("profile ticket is missing"))
        .to_owned();
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("profile proof key is missing"))
        .to_owned();
    let url = format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    );
    let socket = connect_async(url)
        .await
        .unwrap_or_else(|error| panic!("connect profile socket: {error}"))
        .0;
    let mut socket = AuthenticatedTestSocket::new(socket, ticket, proof_key);
    let receipt = socket.subscribe(0).await;
    assert_eq!(receipt["op"], "subscribed");
    socket
}

async fn receive_json<S>(socket: &mut AuthenticatedTestSocket<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    socket
        .receive_json_with_timeout(Duration::from_secs(2))
        .await
}

async fn request_ticket(base_url: &str) -> String {
    let grant = request_ticket_grant(base_url).await;
    grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("profile ticket is missing"))
        .to_owned()
}

async fn request_ticket_grant(base_url: &str) -> Value {
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
    grant
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
    store
        .bootstrap_local_authority("f83f761a-0a6a-4d92-956e-f2d0dadf50c9", "SeiNel")
        .await
        .unwrap_or_else(|error| panic!("bootstrap profile identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000012",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create profile room: {error}"));
}
