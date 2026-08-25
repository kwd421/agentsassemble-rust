use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "room-directory-boundary-host-token-01";

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
            .unwrap_or_else(|error| panic!("join room directory server: {error}"));
    }
}

#[tokio::test]
async fn operator_directory_ticket_is_scope_separated_and_room_creation_is_canonical() {
    let store = fixture().await;
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let server = start(store.clone(), tickets.clone()).await;
    let client = Client::new();

    verify_auth_boundaries(&client, &server, &tickets).await;
    let server_id = read_initial_directory(&client, &server, &tickets).await;
    let room_uid = create_and_retry_room(&client, &server, &tickets, &server_id).await;
    verify_created_room_socket(&server, &tickets, &room_uid).await;
    server.stop().await;
}

#[tokio::test]
async fn central_registration_is_one_use_signed_and_keeps_a_zero_room_authority() {
    let store = zero_room_fixture().await;
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let server = start(store, tickets.clone()).await;
    let client = Client::new();
    let route = format!(
        "{}/api/central-directory/registration-proof",
        server.base_url
    );
    verify_registration_admission(&client, &route, &tickets).await;
    verify_registration_and_zero_room(&client, &server, &route, &tickets).await;
    server.stop().await;
}

async fn verify_registration_admission(client: &Client, route: &str, tickets: &TicketStore) {
    let unauthenticated = client
        .post(route)
        .body("[".repeat(16 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unauthenticated registration: {error}"));
    assert_eq!(unauthenticated.status(), reqwest::StatusCode::FORBIDDEN);

    let preflight = client
        .request(reqwest::Method::OPTIONS, route)
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "POST")
        .header(
            "access-control-request-headers",
            "authorization,content-type",
        )
        .send()
        .await
        .unwrap_or_else(|error| panic!("request registration preflight: {error}"));
    assert!(preflight.status().is_success());
    assert_eq!(
        preflight.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let allowed_headers = preflight.headers()["access-control-allow-headers"]
        .to_str()
        .unwrap_or_else(|error| panic!("decode allowed registration headers: {error}"));
    assert!(allowed_headers.contains("authorization"));
    assert!(allowed_headers.contains("content-type"));
    assert!(!allowed_headers.contains("x-device-token"));

    let generic_operator = issue_operator_ticket(tickets).await;
    let wrong_purpose = client
        .post(route)
        .header("authorization", format!("Bearer {generic_operator}"))
        .json(&json!({"owner_person_id": "per_owner_12345678"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request registration with generic operator: {error}"));
    assert_eq!(wrong_purpose.status(), reqwest::StatusCode::FORBIDDEN);

    let invalid_ticket = issue_registration_ticket(tickets).await;
    let invalid = client
        .post(route)
        .header("authorization", format!("Bearer {invalid_ticket}"))
        .json(&json!({"owner_person_id": "short"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request invalid registration: {error}"));
    assert_eq!(invalid.status(), reqwest::StatusCode::BAD_REQUEST);
    let consumed_invalid = client
        .post(route)
        .header("authorization", format!("Bearer {invalid_ticket}"))
        .json(&json!({"owner_person_id": "per_owner_12345678"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay invalid registration ticket: {error}"));
    assert_eq!(consumed_invalid.status(), reqwest::StatusCode::FORBIDDEN);
}

async fn verify_registration_and_zero_room(
    client: &Client,
    server: &RunningServer,
    route: &str,
    tickets: &TicketStore,
) {
    let owner_person_id = "per_owner_12345678";
    let ticket = issue_registration_ticket(tickets).await;
    let registered = client
        .post(route)
        .header("authorization", format!("Bearer {ticket}"))
        .json(&json!({"owner_person_id": owner_person_id}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request central registration: {error}"));
    assert_eq!(registered.status(), reqwest::StatusCode::OK);
    let registered: Value = registered
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode central registration: {error}"));
    verify_registration_signature(&registered, owner_person_id);

    let replay = client
        .post(route)
        .header("authorization", format!("Bearer {ticket}"))
        .json(&json!({"owner_person_id": owner_person_id}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay central registration ticket: {error}"));
    assert_eq!(replay.status(), reqwest::StatusCode::FORBIDDEN);

    let directory_ticket = issue_operator_ticket(tickets).await;
    let directory: Value = client
        .get(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {directory_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request zero-room directory: {error}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode zero-room directory: {error}"));
    assert_eq!(directory["server_id"], registered["server_id"]);
    assert_eq!(directory["rooms"], json!([]));
    let routes = directory["server_product_surface"]["http_routes"]
        .as_array()
        .unwrap_or_else(|| panic!("server product surface has no routes"));
    assert!(routes.iter().any(|surface| {
        surface["method"] == "POST"
            && surface["path"] == "/api/central-directory/registration-proof"
    }));
    assert!(!routes.iter().any(|surface| {
        matches!(
            surface["path"].as_str(),
            Some("/api/server-info" | "/api/server-info/challenge")
        )
    }));
}

fn verify_registration_signature(registration: &Value, owner_person_id: &str) {
    let public_jwk = &registration["host_public_key_jwk"];
    let proof = &registration["host_registration_proof"];
    assert_eq!(public_jwk["crv"], "Ed25519");
    assert_eq!(public_jwk["kty"], "OKP");
    assert_eq!(proof["owner_person_id"], owner_person_id);
    let canonical_jwk = serde_json::to_vec(public_jwk)
        .unwrap_or_else(|error| panic!("encode canonical host JWK: {error}"));
    assert_eq!(
        registration["host_key_fingerprint"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(canonical_jwk))
    );
    let transcript = format!(
        "AA-HOST-REGISTER-1\n{}\n{}\n{}\n{}",
        registration["server_id"]
            .as_str()
            .unwrap_or_else(|| panic!("registration has no server id")),
        owner_person_id,
        proof["issued_at"]
            .as_i64()
            .unwrap_or_else(|| panic!("registration has no issued-at timestamp")),
        proof["nonce"]
            .as_str()
            .unwrap_or_else(|| panic!("registration has no nonce"))
    );
    let public_key = URL_SAFE_NO_PAD
        .decode(
            public_jwk["x"]
                .as_str()
                .unwrap_or_else(|| panic!("registration has no public key")),
        )
        .unwrap_or_else(|error| panic!("decode registration public key: {error}"));
    let signature = URL_SAFE_NO_PAD
        .decode(
            proof["signature"]
                .as_str()
                .unwrap_or_else(|| panic!("registration has no signature")),
        )
        .unwrap_or_else(|error| panic!("decode registration signature: {error}"));
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(transcript.as_bytes(), &signature)
        .unwrap_or_else(|_| panic!("central registration signature did not verify"));
}

async fn verify_auth_boundaries(client: &Client, server: &RunningServer, tickets: &TicketStore) {
    let unauthorized = client
        .post(format!("{}/api/rooms", server.base_url))
        .body("[".repeat(16 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request unauthenticated create: {error}"));
    assert_eq!(unauthorized.status(), reqwest::StatusCode::UNAUTHORIZED);

    let room_ticket = issue_room_ticket(tickets, "general").await;
    let wrong_scope = client
        .get(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {room_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request directory with room ticket: {error}"));
    assert_eq!(wrong_scope.status(), reqwest::StatusCode::UNAUTHORIZED);
    assert!(connect_room(&server.base_url, &room_ticket).await.is_err());

    let operator_socket_ticket = issue_operator_ticket(tickets).await;
    assert!(
        connect_room(&server.base_url, &operator_socket_ticket)
            .await
            .is_err()
    );
    let consumed_wrong_scope = client
        .get(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {operator_socket_ticket}"))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse rejected operator ticket: {error}"));
    assert_eq!(
        consumed_wrong_scope.status(),
        reqwest::StatusCode::UNAUTHORIZED
    );
}

async fn read_initial_directory(
    client: &Client,
    server: &RunningServer,
    tickets: &TicketStore,
) -> String {
    let list_ticket = issue_operator_ticket(tickets).await;
    let initial = client
        .get(format!(
            "{}/api/rooms?include_archived=true",
            server.base_url
        ))
        .header("authorization", format!("Bearer {list_ticket}"))
        .header("origin", "tauri://localhost")
        .send()
        .await
        .unwrap_or_else(|error| panic!("read initial directory: {error}"));
    assert_eq!(initial.status(), reqwest::StatusCode::OK);
    assert_eq!(
        initial.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let initial: Value = initial
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode initial directory: {error}"));
    let server_id = initial["server_id"]
        .as_str()
        .unwrap_or_else(|| panic!("directory has no server id"))
        .to_owned();
    assert!(uuid::Uuid::parse_str(&server_id).is_ok());
    assert!(
        initial["authority_lineage_id"]
            .as_str()
            .is_some_and(|value| uuid::Uuid::parse_str(value).is_ok())
    );
    assert_eq!(initial["rooms"][0]["room_id"], "general");
    assert!(
        initial["rooms"][0]["room_settings"]["settings_revision"]
            .as_str()
            .is_some_and(|value| value.starts_with("room-settings-v1-"))
    );
    server_id
}

async fn create_and_retry_room(
    client: &Client,
    server: &RunningServer,
    tickets: &TicketStore,
    server_id: &str,
) -> Value {
    let create_ticket = issue_operator_ticket(tickets).await;
    let created = client
        .post(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {create_ticket}"))
        .json(&json!({
            "request_id": "22000000-0000-4000-8000-000000000001",
            "room_id": "project-room",
            "label": "Project Room"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("create canonical room: {error}"));
    assert_eq!(created.status(), reqwest::StatusCode::OK);
    let created: Value = created
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode created room: {error}"));
    assert_eq!(created["server_id"], server_id);
    assert_eq!(created["room"]["label"], "Project Room");
    assert_eq!(created["room"]["origin"], "frontend_room");
    assert!(created["room"].get("room_settings").is_none());
    assert_eq!(created["deduplicated"], false);
    let room_uid = created["room"]["room_uid"].clone();

    let retry_ticket = issue_operator_ticket(tickets).await;
    let retried = client
        .post(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {retry_ticket}"))
        .json(&json!({
            "request_id": "22000000-0000-4000-8000-000000000001",
            "room_id": "project-room",
            "label": "Project Room"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("retry canonical room create: {error}"));
    assert_eq!(retried.status(), reqwest::StatusCode::OK);
    let retried: Value = retried
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode retried room: {error}"));
    assert_eq!(retried["room"]["room_uid"], room_uid);
    assert_eq!(retried["room"]["label"], "Project Room");
    assert_eq!(retried["deduplicated"], true);

    let conflict_ticket = issue_operator_ticket(tickets).await;
    let conflict = client
        .post(format!("{}/api/rooms", server.base_url))
        .header("authorization", format!("Bearer {conflict_ticket}"))
        .json(&json!({
            "request_id": "22000000-0000-4000-8000-000000000001",
            "room_id": "project-room",
            "label": "Renamed Project"
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("request conflicting room replay: {error}"));
    assert_eq!(conflict.status(), reqwest::StatusCode::CONFLICT);
    room_uid
}

async fn verify_created_room_socket(
    server: &RunningServer,
    tickets: &TicketStore,
    room_uid: &Value,
) {
    let new_room_ticket = issue_room_ticket(tickets, "project-room").await;
    let mut socket = connect_room(&server.base_url, &new_room_ticket)
        .await
        .unwrap_or_else(|error| panic!("connect newly created room: {error}"));
    let challenge = "d".repeat(64);
    socket
        .send(Message::Text(
            json!({
                "op":"subscribe",
                "streams":["room_events"],
                "resume_from_seq":0,
                "server_challenge": challenge,
            })
            .to_string()
            .into(),
        ))
        .await
        .unwrap_or_else(|error| panic!("subscribe newly created room: {error}"));
    let receipt = receive_json(&mut socket).await;
    assert_eq!(receipt["op"], "subscribed");
    assert_eq!(receipt["server_challenge"], challenge);
    let snapshot = receive_json(&mut socket).await;
    assert_eq!(snapshot["op"], "snapshot");
    assert_eq!(&snapshot["room"]["room_uid"], room_uid);
    assert_eq!(snapshot["room_settings"]["label"], "Project Room");
    assert_eq!(snapshot["last_seq"], 1);
    assert_eq!(snapshot["events"][0]["type"], "room_created");
    assert_eq!(snapshot["participants"][0]["display_name"], "SeiNel");
    socket
        .close(None)
        .await
        .unwrap_or_else(|error| panic!("close room directory socket: {error}"));
}

async fn start(store: SqliteStore, tickets: TicketStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind room directory runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read room directory address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        tickets,
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate directory host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build directory app state: {error}"));
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve room directory runtime: {error}"));
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
        .unwrap_or_else(|error| panic!("issue operator ticket: {error}"))
        .ticket
}

async fn issue_registration_ticket(tickets: &TicketStore) -> String {
    tickets
        .issue_central_registration(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue registration ticket: {error}"))
        .ticket
}

async fn issue_room_ticket(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue(local_principal(room_id))
        .await
        .unwrap_or_else(|error| panic!("issue room ticket: {error}"))
        .ticket
}

async fn connect_room(
    base_url: &str,
    ticket: &str,
) -> Result<
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    tokio_tungstenite::tungstenite::Error,
> {
    connect_async(format!(
        "{}/ws?ticket={ticket}",
        base_url.replacen("http://", "ws://", 1)
    ))
    .await
    .map(|(socket, _)| socket)
}

async fn receive_json<S>(socket: &mut tokio_tungstenite::WebSocketStream<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let message = socket
        .next()
        .await
        .unwrap_or_else(|| panic!("room directory WebSocket closed"))
        .unwrap_or_else(|error| panic!("read room directory WebSocket: {error}"));
    serde_json::from_slice(&message.into_data())
        .unwrap_or_else(|error| panic!("decode room directory frame: {error}"))
}

async fn fixture() -> SqliteStore {
    let url = format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    );
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open room directory fixture: {error}"));
    store
        .bootstrap_local_authority("149ce88d-61cd-471f-82a1-e03242ff210f", "SeiNel")
        .await
        .unwrap_or_else(|error| panic!("bootstrap room directory identity: {error}"));
    store
        .create_room_for_local_operator(
            "20000000-0000-4000-8000-000000000015",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room directory fixture: {error}"));
    store
}

async fn zero_room_fixture() -> SqliteStore {
    let url = format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    );
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("open zero-room fixture: {error}"));
    store
        .bootstrap_local_authority("249ce88d-61cd-471f-82a1-e03242ff210f", "SeiNel")
        .await
        .unwrap_or_else(|error| panic!("bootstrap zero-room identity: {error}"));
    store
}

fn local_principal(room_id: &str) -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "SeiNel".to_owned(),
        room_id: room_id.to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}
