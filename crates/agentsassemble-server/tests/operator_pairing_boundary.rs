use std::time::Duration;

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

mod support {
    pub mod room_socket_peer;
}
use support::room_socket_peer::RoomSocketPeer;

const ORIGIN: &str = "https://pairing.example.test";
const PROXY: &str = "pairing-test-proxy-secret-00000000001";

fn public(request: RequestBuilder) -> RequestBuilder {
    request
        .header("host", "pairing.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY)
        .header("origin", ORIGIN)
}

async fn operator_ticket(state: &AppState) -> String {
    state
        .tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("operator ticket: {error}"))
        .ticket
}

struct PairingServer {
    state: AppState,
    authority: Value,
    base: String,
    address: std::net::SocketAddr,
    shutdown: CancellationToken,
    running: tokio::task::JoinHandle<Result<(), agentsassemble_server::ServeError>>,
}

impl PairingServer {
    async fn start() -> Self {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("store: {error}"));
        store
            .bootstrap_local_authority("ce447e52-143a-43f1-b45e-d9e580a9c9d6", "Pairing Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap: {error}"));
        store
            .create_room_for_local_operator(
                "94a5e839-21d5-4fc9-b7f3-e63b1834c749",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("room: {error}"));
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .unwrap_or_else(|error| panic!("listen: {error}"));
        let address = listener
            .local_addr()
            .unwrap_or_else(|error| panic!("address: {error}"));
        let state = AppState::local(
            store.clone(),
            TicketStore::new(Duration::from_secs(30), 4096),
            ProviderCatalogService::fixed(ProviderCatalog::default()),
        )
        .await
        .unwrap_or_else(|error| panic!("state: {error}"))
        .with_manual_public_ingress(address, ORIGIN, PROXY)
        .unwrap_or_else(|error| panic!("ingress: {error}"));
        let shutdown = CancellationToken::new();
        let running = tokio::spawn(serve(listener, state.clone(), shutdown.clone()));
        let base = format!("http://{address}");
        let manager = store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("manager: {error}"));
        let authority = json!({"server_id": manager.server_id, "authority_lineage_id": manager.authority_lineage_id, "room_id": "general", "room_uid": manager.room_uid});
        Self {
            state,
            authority,
            base,
            address,
            shutdown,
            running,
        }
    }
}

async fn redeem_with_boundary_checks(
    client: &Client,
    base: &str,
    payload: &Value,
    device: &str,
) -> String {
    let other = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x92; 32]));
    let absent = public(client.post(format!("{base}/api/operator-pairing/redeem")))
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("missing device: {error}"));
    assert_eq!(absent.status(), StatusCode::UNAUTHORIZED);
    let foreign_origin = client
        .post(format!("{base}/api/operator-pairing/redeem"))
        .header("host", "pairing.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY)
        .header("origin", "https://foreign.example.test")
        .header("x-device-token", device)
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("foreign origin: {error}"));
    assert_eq!(foreign_origin.status(), StatusCode::FORBIDDEN);
    let mut session = String::new();
    for _ in 0..2 {
        let redeemed = public(client.post(format!("{base}/api/operator-pairing/redeem")))
            .header("x-device-token", device)
            .json(payload)
            .send()
            .await
            .unwrap_or_else(|error| panic!("redeem: {error}"));
        assert_eq!(redeemed.status(), StatusCode::OK);
        let redeemed: Value = redeemed
            .json()
            .await
            .unwrap_or_else(|error| panic!("redeem response: {error}"));
        assert_eq!(redeemed["operator"], true);
        let bearer = redeemed["session_token"]
            .as_str()
            .unwrap_or_else(|| panic!("session missing"));
        assert!(bearer.starts_with("aops1."));
        if session.is_empty() {
            bearer.clone_into(&mut session);
        } else {
            assert_eq!(session, bearer);
        }
    }
    let foreign = public(client.post(format!("{base}/api/operator-pairing/redeem")))
        .header("x-device-token", &other)
        .json(payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("foreign redeem: {error}"));
    assert_eq!(foreign.status(), StatusCode::UNAUTHORIZED);
    session
}

async fn paired_room_http(
    client: &Client,
    base: &str,
    session: &str,
    device: &str,
    expected: StatusCode,
) {
    for path in [
        "/api/room-settings?room_id=general",
        "/api/room-pins?room_id=general&channel_id=lobby",
        "/api/room-search?room_id=general&channel_id=lobby&q=paired",
    ] {
        // Same-origin browser GETs omit Origin; only proven proxy ingress carries the origin.
        let read = client
            .get(format!("{base}{path}"))
            .header("host", "pairing.example.test")
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", PROXY)
            .header("x-device-token", device)
            .bearer_auth(session)
            .send()
            .await
            .unwrap_or_else(|error| panic!("paired read: {error}"));
        assert_eq!(read.status(), expected, "{path}");
    }
    for (path, body) in [
        (
            "/api/room-settings",
            json!({"room_id": "general", "appearance": {"notifications": "mute"}}),
        ),
        (
            "/api/message-attachments",
            json!({"filename": "paired.txt", "content_type": "text/plain", "data_base64": "cGFpcmVk"}),
        ),
        (
            "/api/attachments",
            json!({"purpose": "room_appearance", "filename": "paired.png", "content_type": "image/png",
                "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="}),
        ),
    ] {
        let write = public(client.post(format!("{base}{path}")))
            .header("x-device-token", device)
            .bearer_auth(session)
            .json(&body)
            .send()
            .await
            .unwrap_or_else(|error| panic!("paired write: {error}"));
        assert_eq!(write.status(), expected, "{path}");
    }
    let profile = public(client.get(format!("{base}/api/user-profile")))
        .header("x-device-token", device)
        .bearer_auth(session)
        .send()
        .await
        .unwrap_or_else(|error| panic!("paired profile boundary: {error}"));
    assert_eq!(profile.status(), StatusCode::UNAUTHORIZED);
    let private = client
        .get(format!("{base}/api/room-settings?room_id=general"))
        .header("origin", ORIGIN)
        .header("x-device-token", device)
        .bearer_auth(session)
        .send()
        .await
        .unwrap_or_else(|error| panic!("private paired request: {error}"));
    assert_eq!(private.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn pairing_http_binds_room_origin_device_and_revokes_active_socket() {
    let PairingServer {
        state,
        authority,
        base,
        address,
        shutdown,
        running,
    } = PairingServer::start().await;
    let client = Client::new();
    let mut outdated = authority.clone();
    outdated["room_uid"] = json!("bf6889dc-0173-4e05-888d-508ef49bb3df");
    let rejected = client
        .post(format!("{base}/api/operator-pairing/create"))
        .bearer_auth(operator_ticket(&state).await)
        .json(&outdated)
        .send()
        .await
        .unwrap_or_else(|error| panic!("stale create: {error}"));
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let created = client
        .post(format!("{base}/api/operator-pairing/create"))
        .bearer_auth(operator_ticket(&state).await)
        .json(&authority)
        .send()
        .await
        .unwrap_or_else(|error| panic!("create: {error}"));
    assert_eq!(created.status(), StatusCode::OK);
    assert_eq!(created.headers()["cache-control"], "private, no-store");
    let created: Value = created
        .json()
        .await
        .unwrap_or_else(|error| panic!("created response: {error}"));
    let pairing_url = created["pairing_url"]
        .as_str()
        .unwrap_or_else(|| panic!("pairing URL missing"));
    let token = pairing_url
        .strip_prefix(&format!("{ORIGIN}/pair?token="))
        .unwrap_or_else(|| panic!("wrong pairing origin"));
    let payload = json!({"pairing_token": token});
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x91; 32]));
    let session = redeem_with_boundary_checks(&client, &base, &payload, &device).await;
    paired_room_http(&client, &base, &session, &device, StatusCode::OK).await;
    let other = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x92; 32]));
    paired_room_http(&client, &base, &session, &other, StatusCode::UNAUTHORIZED).await;
    let wrong = public(client.post(format!("{base}/api/session-tickets/socket")))
        .bearer_auth(&session)
        .header("x-device-token", &other)
        .send()
        .await
        .unwrap_or_else(|error| panic!("foreign exchange: {error}"));
    assert_eq!(wrong.status(), StatusCode::UNAUTHORIZED);
    let exchanged = public(client.post(format!("{base}/api/session-tickets/socket")))
        .bearer_auth(&session)
        .header("x-device-token", &device)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange: {error}"));
    assert_eq!(exchanged.status(), StatusCode::OK);
    let exchanged: Value = exchanged
        .json()
        .await
        .unwrap_or_else(|error| panic!("exchange response: {error}"));
    let ticket = exchanged["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("socket ticket missing"));
    let (wire, _) = tokio_tungstenite::connect_async(format!("ws://{address}/ws?ticket={ticket}"))
        .await
        .unwrap_or_else(|error| panic!("socket: {error}"));
    let mut socket = RoomSocketPeer::new(wire);
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    let revoked = client
        .post(format!("{base}/api/operator-pairing/revoke"))
        .bearer_auth(operator_ticket(&state).await)
        .json(&json!({"authority": authority, "pairing_id": created["pairing_id"]}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("revoke: {error}"));
    assert_eq!(revoked.status(), StatusCode::OK);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), socket.wait_closed())
            .await
            .unwrap_or(false)
    );
    let rejected = public(client.post(format!("{base}/api/operator-pairing/redeem")))
        .header("x-device-token", &device)
        .json(&payload)
        .send()
        .await
        .unwrap_or_else(|error| panic!("revoked retry: {error}"));
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    paired_room_http(&client, &base, &session, &device, StatusCode::UNAUTHORIZED).await;
    shutdown.cancel();
    running
        .await
        .unwrap_or_else(|error| panic!("join: {error}"))
        .unwrap_or_else(|error| panic!("serve: {error}"));
}
