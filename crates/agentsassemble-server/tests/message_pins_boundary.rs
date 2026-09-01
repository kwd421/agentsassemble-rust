use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

struct RunningServer {
    base_url: String,
    store: SqliteStore,
    tickets: TicketStore,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(8), self.task)
            .await
            .unwrap_or_else(|_| panic!("message-pin runtime did not stop"))
            .unwrap_or_else(|error| panic!("join message-pin runtime: {error}"));
    }
}

#[tokio::test]
async fn local_http_pin_lifecycle_uses_canonical_message_projection() {
    let server = start().await;
    let message = send_message(&server.store, "pin-target", "hello pins").await;
    let client = reqwest::Client::new();

    let empty = client
        .get(format!(
            "{}/api/room-pins?room_id=general&channel_id=lobby",
            server.base_url
        ))
        .bearer_auth(issue_read(&server.tickets, "general").await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read empty pins: {error}"));
    assert_eq!(empty.status(), StatusCode::OK);
    assert_eq!(empty.headers()["cache-control"], "private, no-store");
    assert_eq!(json_body(empty).await["pins"], json!([]));

    let write_ticket = issue_write(&server.tickets, "general").await;
    let pinned = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(&write_ticket)
        .json(&json!({
            "room_id": "general",
            "channel_id": "lobby",
            "event_id": message.id,
            "pinned": true
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("pin message: {error}"));
    assert_eq!(pinned.status(), StatusCode::OK);
    let pinned = json_body(pinned).await;
    assert_eq!(pinned["pinned"], true);
    assert_eq!(pinned["pins"][0]["event_id"], message.id);
    assert_eq!(pinned["pins"][0]["channel_id"], "lobby");
    assert_eq!(pinned["pins"][0]["author"], "Host");
    assert_eq!(pinned["pins"][0]["content"], "hello pins");
    assert_eq!(pinned["pins"][0]["attachment_filenames"], json!([]));

    let replay = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(write_ticket)
        .json(&json!({
            "room_id": "general",
            "channel_id": "lobby",
            "event_id": message.id,
            "pinned": false
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay pin ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let unpinned = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(issue_write(&server.tickets, "general").await)
        .json(&json!({
            "room_id": "general",
            "channel_id": "lobby",
            "event_id": message.id,
            "pinned": false
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("unpin message: {error}"));
    assert_eq!(unpinned.status(), StatusCode::OK);
    assert_eq!(json_body(unpinned).await["pins"], json!([]));
    server.stop().await;
}

#[tokio::test]
async fn tcp_boundary_consumes_crossed_authority_and_authorizes_before_body() {
    let server = start().await;
    let client = reqwest::Client::new();

    let wrong_purpose = server
        .tickets
        .issue_preferences_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue crossed preference ticket: {error}"))
        .ticket;
    let crossed = client
        .get(format!(
            "{}/api/room-pins?room_id=general&channel_id=lobby",
            server.base_url
        ))
        .bearer_auth(&wrong_purpose)
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross preference ticket: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);
    let crossed_replay = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(wrong_purpose)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay crossed preference ticket: {error}"));
    assert_eq!(crossed_replay.status(), StatusCode::UNAUTHORIZED);

    let wrong_room = issue_read(&server.tickets, "general").await;
    let rejected_room = client
        .get(format!(
            "{}/api/room-pins?room_id=other&channel_id=lobby",
            server.base_url
        ))
        .bearer_auth(&wrong_room)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read wrong room: {error}"));
    assert_eq!(rejected_room.status(), StatusCode::UNAUTHORIZED);
    let replay = client
        .get(format!(
            "{}/api/room-pins?room_id=general&channel_id=lobby",
            server.base_url
        ))
        .bearer_auth(wrong_room)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay wrong-room ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let stale_identity = server
        .tickets
        .issue_message_pins_write(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            "missing-participant".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue stale-identity ticket: {error}"))
        .ticket;
    let unauthorized_oversized = client
        .post(format!("{}/api/room-pins", server.base_url))
        .bearer_auth(stale_identity)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("x".repeat(8 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send unauthorized oversized body: {error}"));
    assert_eq!(unauthorized_oversized.status(), StatusCode::UNAUTHORIZED);

    server.stop().await;
}

#[tokio::test]
async fn tcp_boundary_rejects_missing_message_targets_for_pin_and_unpin() {
    let server = start().await;
    let client = reqwest::Client::new();
    let room_created = server
        .store
        .snapshot("general", 0, 100)
        .await
        .unwrap_or_else(|error| panic!("read room events: {error}"))
        .events
        .into_iter()
        .find(|event| event.event_type == "room_created")
        .unwrap_or_else(|| panic!("room-created event missing"));
    for (event_id, pinned) in [
        (room_created.id.as_str(), true),
        (room_created.id.as_str(), false),
        ("missing-event", false),
    ] {
        let missing_message = client
            .post(format!("{}/api/room-pins", server.base_url))
            .bearer_auth(issue_write(&server.tickets, "general").await)
            .json(&json!({
                "room_id": "general",
                "channel_id": "lobby",
                "event_id": event_id,
                "pinned": pinned
            }))
            .send()
            .await
            .unwrap_or_else(|error| panic!("mutate missing message: {error}"));
        assert_eq!(missing_message.status(), StatusCode::NOT_FOUND);
    }
    assert_eq!(pin_count(&server.store).await, 0);
    server.stop().await;
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open message-pin store: {error}"));
    store
        .bootstrap_local_authority("78e99dc4-ec2e-48d8-aa18-65cb84ee080b", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap message-pin authority: {error}"));
    store
        .create_room_for_local_operator(
            "f1f514ed-aadc-4039-80c6-d5430645de61",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create message-pin room: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 64);
    let state = AppState::local(
        store.clone(),
        tickets.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build message-pin app state: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind message-pin runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read message-pin address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve message-pin runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        store,
        tickets,
        cancellation,
        task,
    }
}

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

async fn send_message(
    store: &SqliteStore,
    request_id: &str,
    content: &str,
) -> agentsassemble_domain::RoomEvent {
    store
        .execute_message(
            &local_principal(),
            request_id,
            "message.send",
            &json!({"content": content}),
        )
        .await
        .unwrap_or_else(|error| panic!("send message: {error}"))
        .event
}

async fn issue_read(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue_message_pins_read(
            room_id.to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue pin-read ticket: {error}"))
        .ticket
}

async fn issue_write(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue_message_pins_write(
            room_id.to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue pin-write ticket: {error}"))
        .ticket
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode response JSON: {error}"))
}

async fn pin_count(store: &SqliteStore) -> i64 {
    let pins = store
        .local_lobby_message_pins(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("list stored pins: {error}"));
    i64::try_from(pins.len()).unwrap_or(i64::MAX)
}
