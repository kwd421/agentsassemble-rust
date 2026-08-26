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
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::{DateTime, Duration as ChronoDuration, TimeZone, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

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
    let (store, credentials) = fixture().await;
    let server = start(store.clone()).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0xB7; 32]));

    assert_preflight_boundary(
        &client,
        &server.base_url,
        credentials.invite_token(),
        &browser_credential,
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
    )
    .await;
    assert_eq!(first["status"], "admitted");
    assert_eq!(first["request_id"], request_id);
    assert_eq!(first["display_name"], "Boundary Guest");
    assert_eq!(first["meeting_id"], "general");
    assert_eq!(first["invite_scope"], "room");
    assert_eq!(first["client_type"], "browser");
    assert_eq!(first["participant_type"], "human");
    let session_token = first["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("join response has no session token"));
    assert!(session_token.starts_with("aas1."));
    assert_eq!(session_token.len(), 48);

    let retry = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        request_id,
        "Boundary Guest",
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

    server.stop().await;
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
) -> Value {
    let response = client
        .post(format!("{base_url}/api/room-invite/join"))
        .json(&join_body(
            invite_token,
            browser_credential,
            request_id,
            display_name,
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
) -> Value {
    json!({
        "invite_token": invite_token,
        "request_id": request_id,
        "display_name": display_name,
        "avatar_image_url": "",
        "device_token": browser_credential,
        "client_id": "browser-boundary-client",
        "participant_type": "human"
    })
}

async fn fixture() -> (
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
        base_participant_id: "invite-boundary-guest".to_owned(),
        display_name: "Invite Boundary Guest".to_owned(),
        invite_scope: InviteScope::ReadWrite,
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
                max_uses: 1,
                expires_at: draft.expires_at,
                created_at: draft.issued_at,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("persist human invite: {error}"));
    (store, credentials)
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
        TicketStore::new(Duration::from_secs(30), 32),
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
