use std::time::Duration;

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use reqwest::StatusCode;
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "room-preferences-boundary-host-token-0000001";

struct RunningServer {
    base_url: String,
    tickets: TicketStore,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        tokio::time::timeout(Duration::from_secs(8), self.task)
            .await
            .unwrap_or_else(|_| panic!("room preferences runtime did not stop"))
            .unwrap_or_else(|error| panic!("join room preferences runtime: {error}"));
    }
}

#[tokio::test]
async fn preference_http_surface_binds_room_purpose_and_global_transport() {
    let server = start().await;
    let client = reqwest::Client::new();

    let wrong_room_ticket = issue_read(&server.tickets, "general").await;
    let wrong_room = client
        .get(format!(
            "{}/api/room-settings?room_id=other",
            server.base_url
        ))
        .bearer_auth(&wrong_room_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("send wrong-room read: {error}"));
    assert_eq!(wrong_room.status(), StatusCode::UNAUTHORIZED);
    let replay = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(wrong_room_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay wrong-room ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let global_ticket = issue_write(&server.tickets, "general").await;
    let global = client
        .post(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(global_ticket)
        .json(&json!({"room_id": "general", "label": "Changed"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send global HTTP write: {error}"));
    assert_eq!(global.status(), StatusCode::CONFLICT);

    let write_ticket = issue_write(&server.tickets, "general").await;
    let updated = client
        .post(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(write_ticket)
        .json(&json!({
            "room_id": "general",
            "appearance": {"notifications": "mute"},
            "channel_settings": {
                "lobby": {"notifications": "default", "last_read_at": "cursor-1"}
            }
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("write room preferences: {error}"));
    assert_eq!(updated.status(), StatusCode::OK);
    let updated: Value = updated
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode preference response: {error}"));
    assert_eq!(updated["settings"]["label"], "General");
    assert_eq!(updated["settings"]["appearance"]["notifications"], "mute");
    assert_eq!(
        updated["settings"]["channel_settings"]["lobby"]["last_read_at"],
        "cursor-1"
    );

    let read_ticket = issue_read(&server.tickets, "general").await;
    let read = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(read_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read room preferences: {error}"));
    assert_eq!(read.status(), StatusCode::OK);
    let read: Value = read
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode read preferences: {error}"));
    assert_eq!(read, updated);
    server.stop().await;
}

#[tokio::test]
async fn preference_write_authorizes_before_body_and_directory_is_separate() {
    let server = start().await;
    let client = reqwest::Client::new();

    let directory_ticket = server
        .tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue settings directory ticket: {error}"))
        .ticket;
    let directory = client
        .get(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(directory_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read settings directory: {error}"));
    assert_eq!(directory.status(), StatusCode::OK);
    let directory: Value = directory
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode settings directory: {error}"));
    assert_eq!(directory["rooms"][0]["room_id"], "general");

    let room_ticket = issue_read(&server.tickets, "general").await;
    let crossed = client
        .get(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(&room_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross room ticket into directory: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);

    let write_ticket = server
        .tickets
        .issue_preferences_write(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            "missing-participant".to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue stale preference writer: {error}"))
        .ticket;
    let revoked = client
        .post(format!("{}/api/room-settings", server.base_url))
        .bearer_auth(write_ticket)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("x".repeat(20 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("send revoked oversized write: {error}"));
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    server.stop().await;
}

async fn issue_read(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue_preferences_read(
            room_id.to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue preference read ticket: {error}"))
        .ticket
}

async fn issue_write(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue_preferences_write(
            room_id.to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue preference write ticket: {error}"))
        .ticket
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open room preferences store: {error}"));
    store
        .bootstrap_local_authority(
            "4fce6f73-edff-4832-a4b7-c7a43e9006af",
            "Preferences Operator",
        )
        .await
        .unwrap_or_else(|error| panic!("bootstrap preference authority: {error}"));
    store
        .create_room_for_local_operator(
            "f237462e-d761-4863-9f89-c25b74ad26d2",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create preference room: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 64);
    let state = AppState::local(
        store.clone(),
        tickets.clone(),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate preference host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build preference app state: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind preference runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read preference runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve preference runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        tickets,
        cancellation,
        task,
    }
}
