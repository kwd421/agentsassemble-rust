use std::time::Duration;

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{
    AppState, HostSecret, HumanInviteCredentialAuthority, TicketStore,
    VerifiedHumanInviteCredential, serve,
};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "human-invite-manager-host-token-0000001";
const PUBLIC_ORIGIN: &str = "https://public.example.test";
const PROXY_SECRET: &str = "human-invite-manager-proxy-secret-000001";

struct RunningServer {
    base_url: String,
    tickets: TicketStore,
    store: SqliteStore,
    credentials: HumanInviteCredentialAuthority,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(8), self.task)
            .await
            .unwrap_or_else(|_| panic!("human invite manager runtime did not stop"))
            .unwrap_or_else(|error| panic!("join human invite manager runtime: {error}"));
    }
}

#[tokio::test]
async fn manager_create_and_revoke_use_one_ready_ingress_and_current_room_authority() {
    let server = start(true).await;
    let client = reqwest::Client::new();
    let create_ticket = issue_create(&server, "general").await;
    let created = client
        .post(format!("{}/api/room-invite/create", server.base_url))
        .bearer_auth(create_ticket)
        .json(&json!({
            "meeting_id": "general",
            "display_name": "",
            "invite_scope": "read_only",
            "ttl_seconds": 90,
            "max_uses": -7
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create human invite: {error}"));
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(created.headers()["cache-control"], "private, no-store");
    let created: Value = created
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode created invite: {error}"));
    assert_eq!(created["display_name"], "Guest");
    assert_eq!(created["invite_scope"], "read_only");
    assert_eq!(created["permission_mode"], "meeting_read_only");
    assert_eq!(created["max_uses"], 0);
    assert_eq!(created["room_url"], server.base_url);
    let join_code = created["join_code"]
        .as_str()
        .unwrap_or_else(|| panic!("created invite has no join code"));
    assert_eq!(
        created["join_url"],
        format!("{PUBLIC_ORIGIN}/join?token={join_code}")
    );
    let token = created["invite_token"]
        .as_str()
        .unwrap_or_else(|| panic!("created invite has no signed token"));
    let VerifiedHumanInviteCredential::Signed { claims, .. } = server
        .credentials
        .authenticate(token)
        .unwrap_or_else(|error| panic!("authenticate created invite: {error}"))
    else {
        panic!("created invite token was not signed");
    };
    assert_eq!(claims.room_url, server.base_url);
    assert_eq!(claims.public_room_url, PUBLIC_ORIGIN);
    assert_eq!(claims.base_participant_id, created["agent_id"]);

    let invite_id = created["invite_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created invite has no id"));
    let stored = server
        .store
        .list_human_invites()
        .await
        .unwrap_or_else(|error| panic!("list created invite: {error}"));
    assert_eq!(stored[0].invite_id, invite_id);

    let revoke_ticket = issue_revoke(&server, "general").await;
    let revoked = client
        .post(format!("{}/api/room-invite/revoke", server.base_url))
        .bearer_auth(revoke_ticket)
        .json(&json!({"meeting_id": "general", "invite_id": invite_id}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("revoke human invite: {error}"));
    assert_eq!(revoked.status(), StatusCode::OK);
    assert!(
        server
            .store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("read revoked invite: {error}"))[0]
            .revoked
    );
    server.stop().await;
}

#[tokio::test]
async fn create_consumes_exact_ticket_before_body_and_not_ready_writes_nothing() {
    let ready = start(true).await;
    let client = reqwest::Client::new();
    let crossed_ticket = issue_revoke(&ready, "general").await;
    let crossed = client
        .post(format!("{}/api/room-invite/create", ready.base_url))
        .bearer_auth(&crossed_ticket)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("x".repeat(16 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross revoke ticket into create: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);
    let replay = client
        .post(format!("{}/api/room-invite/revoke", ready.base_url))
        .bearer_auth(crossed_ticket)
        .json(&json!({"meeting_id": "general", "invite_id": "ffffffffffffffff"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay crossed ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let wrong_room_ticket = issue_create(&ready, "general").await;
    let wrong_room = client
        .post(format!("{}/api/room-invite/create", ready.base_url))
        .bearer_auth(wrong_room_ticket)
        .json(&json!({
            "meeting_id": "other",
            "invite_scope": "room",
            "ttl_seconds": 60,
            "max_uses": 1
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross create ticket into another room: {error}"));
    assert_eq!(wrong_room.status(), StatusCode::UNAUTHORIZED);

    let client_owned_identity = issue_create(&ready, "general").await;
    let rejected_identity = client
        .post(format!("{}/api/room-invite/create", ready.base_url))
        .bearer_auth(client_owned_identity)
        .json(&json!({
            "meeting_id": "general",
            "agent_id": "client-chosen",
            "ttl_seconds": 60
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("submit client-owned invite identity: {error}"));
    assert_eq!(rejected_identity.status(), StatusCode::BAD_REQUEST);
    assert!(
        ready
            .store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("list rejected ready invites: {error}"))
            .is_empty()
    );
    ready.stop().await;

    let unavailable = start(false).await;
    let ticket = issue_create(&unavailable, "general").await;
    let rejected = client
        .post(format!("{}/api/room-invite/create", unavailable.base_url))
        .bearer_auth(ticket)
        .json(&json!({
            "meeting_id": "general",
            "invite_scope": "room",
            "ttl_seconds": 60,
            "max_uses": 1
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create without ready ingress: {error}"));
    assert_eq!(rejected.status(), StatusCode::CONFLICT);
    let rejected: Value = rejected
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode not-ready rejection: {error}"));
    assert_eq!(rejected["error"]["code"], "public_ingress_not_ready");
    assert!(
        unavailable
            .store
            .list_human_invites()
            .await
            .unwrap_or_else(|error| panic!("list not-ready invites: {error}"))
            .is_empty()
    );
    unavailable.stop().await;
}

async fn issue_create(server: &RunningServer, room_id: &str) -> String {
    let authority = server
        .store
        .authorize_local_room_manager(
            room_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize invite-create manager: {error}"));
    server
        .tickets
        .issue_human_invite_create(authority)
        .await
        .unwrap_or_else(|error| panic!("issue invite-create ticket: {error}"))
        .ticket
}

async fn issue_revoke(server: &RunningServer, room_id: &str) -> String {
    let authority = server
        .store
        .authorize_local_room_manager(
            room_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize invite-revoke manager: {error}"));
    server
        .tickets
        .issue_human_invite_revoke(authority)
        .await
        .unwrap_or_else(|error| panic!("issue invite-revoke ticket: {error}"))
        .ticket
}

async fn start(ready: bool) -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open human invite manager store: {error}"));
    store
        .bootstrap_local_authority("6f3ba4cb-d88d-46b3-aac5-67808566c7fd", "Invite Manager")
        .await
        .unwrap_or_else(|error| panic!("bootstrap human invite manager: {error}"));
    store
        .create_room_for_local_operator(
            "cf1d78e1-9b30-40d5-8da8-eb796301d937",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create human invite manager room: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind human invite manager runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read human invite manager address: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 64);
    let mut state = AppState::local(
        store.clone(),
        tickets.clone(),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate human invite manager host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build human invite manager state: {error}"));
    if ready {
        state = state
            .with_manual_public_ingress(address, PUBLIC_ORIGIN, PROXY_SECRET)
            .unwrap_or_else(|error| panic!("configure human invite manager ingress: {error}"));
    }
    let credentials = state.human_invite_credentials.clone();
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve human invite manager runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        tickets,
        store,
        credentials,
        cancellation,
        task,
    }
}
