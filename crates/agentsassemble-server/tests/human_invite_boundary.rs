use std::time::Duration;

use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::{NewHumanInvite, SqliteStore};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{
    AppState, HostSecret, HumanInviteCredentialAuthority, HumanInviteCredentialDraft, TicketStore,
    serve,
};
use base64::{
    Engine as _,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::connect_async;
use tokio_util::sync::CancellationToken;

mod support {
    pub mod human_profile_exchange;
    pub mod subscription_proof;
}

use support::{
    human_profile_exchange::{
        assert_avatar_available, assert_profile_exchange_boundary, issue_profile_ticket,
    },
    subscription_proof::AuthenticatedTestSocket,
};

const HOST_TOKEN: &str = "human-invite-boundary-host-token-0001";

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
            .unwrap_or_else(|error| panic!("join human invite server: {error}"));
    }
}

#[tokio::test]
async fn preflight_and_join_preserve_bounded_credentials_and_exact_retry() {
    let (store, credentials) = fixture(InviteScope::ReadWrite).await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB7; 32]));
    let other_browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB8; 32]));

    assert_preflight_boundary(
        &client,
        &server.base_url,
        credentials.invite_token(),
        &browser_credential,
    )
    .await;
    let (avatar, other_browser_avatar) = prepare_prejoin_avatar_flow(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        &other_browser_credential,
    )
    .await;
    let request_id = "123e4567-e89b-12d3-a456-426614174000";
    let first = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        request_id,
        "Boundary Guest",
        &avatar,
    )
    .await;
    assert_eq!(first["status"], "admitted");
    assert_eq!(first["request_id"], request_id);
    assert_eq!(first["display_name"], "Boundary Guest");
    assert_eq!(first["meeting_id"], "general");
    assert_eq!(first["invite_scope"], "room");
    assert_eq!(first["client_type"], "browser");
    assert_eq!(first["participant_type"], "human");
    assert_eq!(first["avatar_image_url"], avatar);
    assert_session_server_surface(&first);
    let session_token = canonical_session_token(&first);
    let replacement_avatar =
        assert_profile_exchange_boundary(&client, &server.base_url, &store, session_token, &avatar)
            .await;
    assert_session_socket_boundary(&client, &server.base_url, session_token).await;

    let retry = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        request_id,
        "Boundary Guest",
        &avatar,
    )
    .await;
    assert!(retry == first, "exact retry response changed");
    assert_eq!(
        store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("inspect invite after retry: {error}"))[0]
            .use_count,
        1
    );

    let conflict = client
        .post(format!("{}/api/room-invite/join", server.base_url))
        .json(&join_body(
            credentials.join_code(),
            &browser_credential,
            request_id,
            "Changed Guest",
            &avatar,
        ))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request conflicting exact join: {error}"));
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    let conflict: Value = conflict
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode conflicting exact join: {error}"));
    assert_eq!(conflict["code"], "idempotency_conflict");
    assert_eq!(
        store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("inspect invite after conflict: {error}"))[0]
            .use_count,
        1
    );
    let replaced = client
        .get(format!("{}{}", server.base_url, avatar))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read replaced admission avatar: {error}"));
    assert_eq!(replaced.status(), reqwest::StatusCode::NOT_FOUND);
    assert_avatar_available(&client, &server.base_url, &replacement_avatar).await;
    assert_avatar_available(&client, &server.base_url, &other_browser_avatar).await;

    server.stop().await;
}

#[tokio::test]
async fn read_only_session_patches_profile_but_cannot_upload_an_avatar() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xC7; 32]));
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        "223e4567-e89b-12d3-a456-426614174000",
        "Read Only Guest",
        "",
    )
    .await;
    assert_eq!(admitted["invite_scope"], "read_only");
    let session_token = admitted["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("read-only admission has no session token"));

    let update_ticket = issue_profile_ticket(&client, &server.base_url, session_token).await;
    let updated: Value = client
        .post(format!("{}/api/user-profile", server.base_url))
        .header("authorization", format!("Bearer {update_ticket}"))
        .json(&json!({"custom_status": "Still a person profile"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("patch read-only person profile: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only person profile: {error}"));
    assert_eq!(
        updated["profile"]["custom_status"],
        "Still a person profile"
    );

    let upload_ticket = issue_profile_ticket(&client, &server.base_url, session_token).await;
    let rejected = client
        .post(format!("{}/api/attachments", server.base_url))
        .header("authorization", format!("Bearer {upload_ticket}"))
        .body("{")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject read-only avatar upload: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::FORBIDDEN);
    let rejected: Value = rejected
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read-only upload rejection: {error}"));
    assert_eq!(rejected["code"], "session_read_only");

    let mut socket = open_session_socket(&client, &server.base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "read-only-message-1",
            "action": "message.send",
            "payload": {"content": "must not commit"}
        }))
        .await;
    let nack = socket.receive_json().await;
    assert_eq!(nack["op"], "nack");
    assert_eq!(nack["error"]["code"], "permission_denied");
    socket.close().await;
    server.stop().await;
}

#[tokio::test]
async fn replacement_admission_closes_an_idle_human_socket() {
    let (store, first_invite) = fixture_with_max_uses(InviteScope::ReadWrite, 5).await;
    let second_invite = persist_invite(
        &store,
        InviteScope::ReadWrite,
        5,
        "replacement-boundary-guest",
        "Replacement Boundary Guest",
    )
    .await;
    let server = start(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xD7; 32]));
    let first = join(
        &client,
        &server.base_url,
        first_invite.join_code(),
        &browser_credential,
        "323e4567-e89b-12d3-a456-426614174000",
        "First Reusable Guest",
        "",
    )
    .await;
    let first_session = canonical_session_token(&first);
    let mut socket = open_session_socket(&client, &server.base_url, first_session).await;

    let second = join(
        &client,
        &server.base_url,
        second_invite.join_code(),
        &browser_credential,
        "423e4567-e89b-12d3-a456-426614174000",
        "Replacement Reusable Guest",
        "",
    )
    .await;
    assert_ne!(canonical_session_token(&second), first_session);
    assert!(
        socket.wait_closed().await,
        "replaced idle socket stayed open"
    );

    let rejected = client
        .post(format!("{}/api/session-tickets/socket", server.base_url))
        .header("authorization", format!("Bearer {first_session}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("recheck replaced session exchange: {error}"));
    assert_eq!(rejected.status(), reqwest::StatusCode::UNAUTHORIZED);
    server.stop().await;
}

async fn assert_session_socket_boundary(client: &Client, base_url: &str, session_token: &str) {
    let mut socket = open_session_socket(client, base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "human-socket-message-1",
            "action": "message.send",
            "payload": {"content": "human socket boundary"}
        }))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!(
        [&first, &second].iter().any(|frame| {
            frame["op"] == "ack" && frame["request_id"] == "human-socket-message-1"
        })
    );
    assert!([&first, &second].iter().any(|frame| {
        frame["op"] == "event"
            && frame["events"].as_array().is_some_and(|events| {
                events
                    .iter()
                    .any(|event| event["content"] == "human socket boundary")
            })
    }));
    socket.close().await;
}

async fn open_session_socket(
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

fn canonical_session_token(admission: &Value) -> &str {
    let token = admission["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("join response has no session token"));
    assert!(token.starts_with("aas1."));
    assert_eq!(token.len(), 48);
    token
}

fn assert_session_server_surface(admission: &Value) {
    for field in ["server_id", "authority_lineage_id"] {
        assert!(
            admission[field]
                .as_str()
                .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok()),
            "{field} is not a UUID"
        );
    }
    assert_eq!(
        admission["server_product_surface"]["websocket_streams"],
        json!(["room_events"])
    );
}

async fn prepare_prejoin_avatar_flow(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
    other_browser_credential: &str,
) -> (String, String) {
    let invalid_ticket = client
        .post(format!("{base_url}/api/attachments"))
        .header("authorization", "Bearer invalid-profile-ticket")
        .body("{")
        .send()
        .await
        .unwrap_or_else(|error| panic!("request prejoin upload with invalid ticket: {error}"));
    assert_eq!(invalid_ticket.status(), reqwest::StatusCode::UNAUTHORIZED);

    let unknown_join_code = format!("aaj1_{}", URL_SAFE_NO_PAD.encode([0xE1; 24]));
    let large_valid_base64 = STANDARD.encode(vec![0_u8; 10 * 1024 * 1024]);
    let unknown_invite = client
        .post(format!("{base_url}/api/attachments"))
        .json(&json!({
            "purpose": "profile_avatar",
            "invite_token": unknown_join_code,
            "device_token": browser_credential,
            "filename": "unknown.png",
            "content_type": "image/png",
            "data_base64": large_valid_base64,
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unknown-invite prejoin upload: {error}"));
    assert_eq!(unknown_invite.status(), reqwest::StatusCode::FORBIDDEN);
    let unknown_invite: Value = unknown_invite
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode unknown-invite prejoin rejection: {error}"));
    assert_eq!(unknown_invite["code"], "invite_invalid");

    let other_browser_avatar =
        upload_prejoin_avatar(client, base_url, invite_token, other_browser_credential).await;
    let replaced_avatar =
        upload_prejoin_avatar(client, base_url, invite_token, browser_credential).await;
    let avatar = upload_prejoin_avatar(client, base_url, invite_token, browser_credential).await;
    let replaced = client
        .get(format!("{base_url}{replaced_avatar}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("read replaced prejoin avatar: {error}"));
    assert_eq!(replaced.status(), reqwest::StatusCode::NOT_FOUND);
    assert_avatar_available(client, base_url, &avatar).await;
    assert_avatar_available(client, base_url, &other_browser_avatar).await;
    (avatar, other_browser_avatar)
}

async fn assert_preflight_boundary(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
) {
    let route = format!("{base_url}/api/room-invite/admission");
    let missing_browser = client
        .post(&route)
        .json(&json!({"invite_token": invite_token}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request preflight without browser credential: {error}"));
    assert_eq!(missing_browser.status(), reqwest::StatusCode::BAD_REQUEST);

    let invalid_invite: Value = client
        .post(&route)
        .header("x-device-token", browser_credential)
        .json(&json!({"invite_token": "not-an-invite"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invalid invite preflight: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode invalid invite preflight: {error}"));
    assert_eq!(invalid_invite["status"], "invite_invalid");
    assert_eq!(invalid_invite["can_auto_join"], false);

    let preflight = client
        .post(&route)
        .header("origin", "tauri://localhost")
        .header("x-device-token", browser_credential)
        .json(&json!({"invite_token": invite_token}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invite preflight: {error}"));
    assert_eq!(preflight.status(), reqwest::StatusCode::OK);
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    assert_eq!(preflight.headers()["cache-control"], "private, no-store");
    let preflight: Value = preflight
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode invite preflight: {error}"));
    assert_eq!(preflight["status"], "profile_required");
    assert_eq!(preflight["room_id"], "general");
    assert_eq!(preflight["room_label"], "General");
    assert_eq!(preflight["invite_scope"], "room");
    assert_eq!(preflight["can_auto_join"], false);

    let cors = client
        .request(reqwest::Method::OPTIONS, &route)
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type,x-device-token",
        )
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invite CORS preflight: {error}"));
    assert!(cors.status().is_success());
    let allowed = cors.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap_or_else(|error| panic!("decode invite CORS headers: {error}"));
    assert!(allowed.contains("authorization"));
    assert!(allowed.contains("content-type"));
    assert!(allowed.contains("x-device-token"));
}

async fn join(
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

fn join_body(
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

async fn upload_prejoin_avatar(
    client: &Client,
    base_url: &str,
    invite_token: &str,
    browser_credential: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/attachments"))
        .json(&json!({
            "purpose": "profile_avatar",
            "invite_token": invite_token,
            "device_token": browser_credential,
            "filename": "../guest.webp",
            "content_type": "image/png",
            "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload prejoin avatar: {error}"));
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let response: Value = response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode prejoin avatar upload: {error}"));
    assert_eq!(response["attachment"]["filename"], "guest.png");
    assert_eq!(response["attachment"]["content_type"], "image/png");
    response["attachment"]["url"]
        .as_str()
        .unwrap_or_else(|| panic!("prejoin avatar URL is missing"))
        .to_owned()
}

async fn fixture(
    invite_scope: InviteScope,
) -> (
    SqliteStore,
    agentsassemble_server::IssuedHumanInviteCredentials,
) {
    fixture_with_max_uses(invite_scope, 1).await
}

async fn fixture_with_max_uses(
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

async fn persist_invite(
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

fn canonical_now() -> DateTime<Utc> {
    Utc.timestamp_micros(Utc::now().timestamp_micros())
        .single()
        .unwrap_or_else(|| panic!("current timestamp must be valid"))
}

async fn start(store: SqliteStore) -> RunningServer {
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
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate human invite host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build human invite app state: {error}"));
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve human invite runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        cancellation,
        task,
    }
}
