#![allow(dead_code)] // Integration binaries exercise different invite-boundary helpers.

use std::time::Duration;

use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::{NewHumanInvite, SqliteStore};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{
    AppState, HumanInviteCredentialAuthority, HumanInviteCredentialDraft, RoomRuntime, TicketStore,
    serve,
};
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::connect_async;
use tokio_util::sync::CancellationToken;

use super::subscription_proof::AuthenticatedTestSocket;

pub struct RunningServer {
    pub base_url: String,
    state: AppState,
    rooms: RoomRuntime,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    pub fn state(&self) -> &AppState {
        &self.state
    }

    pub fn rooms(&self) -> &RoomRuntime {
        &self.rooms
    }

    pub async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("join human invite server: {error}"));
    }
}

pub async fn fixture(
    invite_scope: InviteScope,
) -> (
    SqliteStore,
    agentsassemble_server::IssuedHumanInviteCredentials,
) {
    fixture_with_max_uses(invite_scope, 1).await
}

pub async fn fixture_with_max_uses(
    invite_scope: InviteScope,
    max_uses: i64,
) -> (
    SqliteStore,
    agentsassemble_server::IssuedHumanInviteCredentials,
) {
    let url = format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    );
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open human invite fixture: {error}"));
    store
        .bootstrap_local_authority("325bb3a2-4964-42a2-8490-afcf6f99b164", "SeiNel")
        .await
        .unwrap_or_else(|error| panic!("bootstrap human invite fixture: {error}"));
    store
        .create_room_for_local_operator(
            "78ba8995-4ef7-4584-9d73-22caee9b3ac5",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create human invite room: {error}"));
    let credentials = persist_invite(
        &store,
        invite_scope,
        max_uses,
        "invite-boundary-guest",
        "Invite Boundary Guest",
    )
    .await;
    (store, credentials)
}

pub async fn persist_invite(
    store: &SqliteStore,
    invite_scope: InviteScope,
    max_uses: i64,
    base_participant_id: &str,
    display_name: &str,
) -> agentsassemble_server::IssuedHumanInviteCredentials {
    let identity = store
        .host_identity()
        .await
        .unwrap_or_else(|error| panic!("load human invite host identity: {error}"));
    let authority = HumanInviteCredentialAuthority::from_persistent(&identity);
    let issued_at = canonical_now();
    let draft = HumanInviteCredentialDraft {
        room_url: "http://127.0.0.1:8765".to_owned(),
        public_room_url: String::new(),
        room_id: "general".to_owned(),
        base_participant_id: base_participant_id.to_owned(),
        display_name: display_name.to_owned(),
        invite_scope,
        issued_at,
        expires_at: issued_at + ChronoDuration::days(1),
    };
    let credentials = authority
        .issue(&draft)
        .unwrap_or_else(|error| panic!("issue human invite credentials: {error}"));
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize human invite manager: {error}"));
    store
        .create_human_invite_for_local_manager(
            &manager,
            NewHumanInvite {
                signed_token_fingerprint: *credentials.signed_token_fingerprint(),
                join_code_fingerprint: *credentials.join_code_fingerprint(),
                base_participant_id: draft.base_participant_id,
                display_name: draft.display_name,
                invite_scope: draft.invite_scope,
                max_uses,
                expires_at: draft.expires_at,
                created_at: draft.issued_at,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("persist human invite: {error}"));
    credentials
}

pub async fn join(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
    request_id: &str,
    display_name: &str,
    avatar_image_url: &str,
) -> Value {
    let response = client
        .post(format!("{base_url}/api/room-invite/join"))
        .json(&join_body(
            invite_token,
            browser_credential,
            request_id,
            display_name,
            avatar_image_url,
        ))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request human invite join: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode human invite join: {error}"))
}

pub fn join_body(
    invite_token: &str,
    browser_credential: &str,
    request_id: &str,
    display_name: &str,
    avatar_image_url: &str,
) -> Value {
    json!({
        "invite_token": invite_token,
        "request_id": request_id,
        "display_name": display_name,
        "avatar_image_url": avatar_image_url,
        "device_token": browser_credential,
        "client_id": "browser-boundary-client",
        "participant_type": "human"
    })
}

pub fn canonical_session_token(admission: &Value) -> &str {
    let token = admission["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("join response has no session token"));
    assert!(token.starts_with("aas1."));
    assert_eq!(token.len(), 48);
    token
}

pub async fn open_session_socket(
    client: &Client,
    base_url: &str,
    session_token: &str,
) -> AuthenticatedTestSocket<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let response = client
        .post(format!("{base_url}/api/session-tickets/socket"))
        .header("authorization", format!("Bearer {session_token}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange human socket ticket: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let grant: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode human socket ticket: {error}"));
    let ticket = grant["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("human socket ticket is missing"))
        .to_owned();
    let proof_key = grant["server_proof_key"]
        .as_str()
        .unwrap_or_else(|| panic!("human socket proof key is missing"))
        .to_owned();
    let socket = connect_async(format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    ))
    .await
    .unwrap_or_else(|error| panic!("connect human room socket: {error}"))
    .0;
    let mut socket = AuthenticatedTestSocket::new(socket, ticket.clone(), proof_key);
    let receipt = socket.subscribe(0).await;
    assert_eq!(receipt["op"], "subscribed");
    assert_eq!(receipt["room_id"], "general");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    let replay = connect_async(format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    ))
    .await;
    assert!(replay.is_err(), "one-use human socket ticket was replayed");
    socket
}

fn canonical_now() -> DateTime<Utc> {
    Utc.timestamp_micros(Utc::now().timestamp_micros())
        .single()
        .unwrap_or_else(|| panic!("current timestamp must be valid"))
}

pub async fn start(store: SqliteStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind human invite runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read human invite address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 4_096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build human invite app state: {error}"));
    let server_state = state.clone();
    let rooms = state.rooms.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve human invite runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        state: server_state,
        rooms,
        cancellation,
        task,
    }
}
