use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}

use support::human_invite::{canonical_session_token, fixture, join, start as start_invite};

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
        self.task
            .await
            .unwrap_or_else(|error| panic!("join message-search runtime: {error}"));
    }
}

#[tokio::test]
async fn local_tcp_search_and_context_are_exact() {
    let server = start().await;
    let mut messages = Vec::new();
    for index in 0..35 {
        messages.push(
            send_message(
                &server.store,
                &format!("tcp-search-{index}"),
                &format!("tcp needle {index}"),
            )
            .await,
        );
    }
    let client = Client::new();
    let ticket = issue_search(&server.tickets, "general").await;
    let first = client
        .get(format!(
            "{}/api/room-search?room_id=general&channel_id=all&q=tcp%20needle",
            server.base_url
        ))
        .bearer_auth(&ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("search over TCP: {error}"));
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["cache-control"], "private, no-store");
    let first = json_body(first).await;
    assert_eq!(first["results"].as_array().map(Vec::len), Some(30));
    assert_eq!(first["results"][0]["event_id"], messages[34].id);
    assert_eq!(
        first["results"][0]["participant_id"],
        LOCAL_OPERATOR_PARTICIPANT_ID
    );
    assert!(
        first["next_cursor"]
            .as_str()
            .is_some_and(|value| !value.is_empty())
    );
    let replay = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay search ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let context = client
        .get(format!(
            "{}/api/room-search/context?room_id=general&channel_id=lobby&event_id={}",
            server.base_url, messages[20].id
        ))
        .bearer_auth(issue_search(&server.tickets, "general").await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read context over TCP: {error}"));
    assert_eq!(context.status(), StatusCode::OK);
    let context = json_body(context).await;
    assert_eq!(context["channel_id"], "lobby");
    assert_eq!(context["event_id"], messages[20].id);
    assert_eq!(context["events"].as_array().map(Vec::len), Some(30));
    server.stop().await;
}

#[tokio::test]
async fn malformed_query_and_oversized_body_consume_the_ticket() {
    let server = start().await;
    let client = Client::new();
    let malformed_ticket = issue_search(&server.tickets, "general").await;
    let malformed = client
        .get(format!(
            "{}/api/room-search?room_id=general&unknown=value",
            server.base_url
        ))
        .bearer_auth(&malformed_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("send malformed search query: {error}"));
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let consumed = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(malformed_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay malformed-query ticket: {error}"));
    assert_eq!(consumed.status(), StatusCode::UNAUTHORIZED);

    let oversized_ticket = issue_search(&server.tickets, "general").await;
    let oversized = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(&oversized_ticket)
        .body(vec![b'x'; 4 * 1024 + 1])
        .send()
        .await
        .unwrap_or_else(|error| panic!("send oversized search body: {error}"));
    assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
    let oversized_replay = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(oversized_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay oversized-body ticket: {error}"));
    assert_eq!(oversized_replay.status(), StatusCode::UNAUTHORIZED);
    server.stop().await;
}

#[tokio::test]
async fn crossed_purpose_and_wrong_room_consume_the_ticket() {
    let server = start().await;
    let client = Client::new();
    let wrong_purpose = server
        .tickets
        .issue_preferences_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue crossed ticket: {error}"))
        .ticket;
    let crossed = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(&wrong_purpose)
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross ticket purpose: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);
    let crossed_replay = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(wrong_purpose)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay crossed ticket: {error}"));
    assert_eq!(crossed_replay.status(), StatusCode::UNAUTHORIZED);

    let wrong_room = issue_search(&server.tickets, "general").await;
    let rejected = client
        .get(format!(
            "{}/api/room-search?room_id=other&q=tcp",
            server.base_url
        ))
        .bearer_auth(&wrong_room)
        .send()
        .await
        .unwrap_or_else(|error| panic!("search wrong room: {error}"));
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let rejected_replay = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=tcp",
            server.base_url
        ))
        .bearer_auth(wrong_room)
        .send()
        .await
        .unwrap_or_else(|error| panic!("replay wrong-room ticket: {error}"));
    assert_eq!(rejected_replay.status(), StatusCode::UNAUTHORIZED);
    server.stop().await;
}

#[tokio::test]
async fn read_only_session_search_is_direct_reusable_and_current() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let message = send_message(&store, "remote-search-message", "remote searchable").await;
    let server = start_invite(store).await;
    let client = Client::new();
    let browser_credential = format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x47; 32]));
    let admission = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential,
        "b23e4567-e89b-12d3-a456-426614174000",
        "Search Reader",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admission);

    let retired_exchange = client
        .post(format!(
            "{}/api/session-tickets/message-search-read",
            server.base_url
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("call retired search exchange: {error}"));
    assert_eq!(retired_exchange.status(), StatusCode::NOT_FOUND);

    let readable = client
        .get(format!(
            "{}/api/room-search?room_id=general&channel_id=lobby&q=remote",
            server.base_url
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("search through read-only session: {error}"));
    assert_eq!(readable.status(), StatusCode::OK);
    assert_eq!(
        json_body(readable).await["results"][0]["content"],
        "remote searchable"
    );

    let wrong_room = client
        .get(format!(
            "{}/api/room-search?room_id=other&q=remote",
            server.base_url
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("search wrong room through session: {error}"));
    assert_eq!(wrong_room.status(), StatusCode::UNAUTHORIZED);

    let context = client
        .get(format!(
            "{}/api/room-search/context?room_id=general&channel_id=lobby&event_id={}",
            server.base_url, message.id
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read context through reusable session: {error}"));
    assert_eq!(context.status(), StatusCode::OK);
    assert_eq!(json_body(context).await["event_id"], message.id);

    let malformed = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=remote",
            server.base_url
        ))
        .bearer_auth("aas1.malformed")
        .send()
        .await
        .unwrap_or_else(|error| panic!("send malformed session bearer: {error}"));
    assert_eq!(malformed.status(), StatusCode::UNAUTHORIZED);

    let left = client
        .post(format!("{}/api/room-invite/leave", server.base_url))
        .bearer_auth(session_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("leave before session reuse: {error}"));
    assert_eq!(left.status(), StatusCode::OK);
    let revoked = client
        .get(format!(
            "{}/api/room-search?room_id=general&q=remote",
            server.base_url
        ))
        .bearer_auth(session_token)
        .body(vec![b'x'; 4 * 1024 + 1])
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse departed session: {error}"));
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    server.stop().await;
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open search store: {error}"));
    store
        .bootstrap_local_authority("9af4f32e-7cd9-4bd3-8978-b240495603a7", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap search authority: {error}"));
    store
        .create_room_for_local_operator(
            "6479ed57-fe63-4574-978e-111a2ab866c2",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create search room: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 64);
    let state = AppState::local(
        store.clone(),
        tickets.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build search app state: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind search runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read search address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve search runtime: {error}"));
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
        .unwrap_or_else(|error| panic!("send search message: {error}"))
        .event
}

async fn issue_search(tickets: &TicketStore, room_id: &str) -> String {
    tickets
        .issue_message_search_read(
            room_id.to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue search ticket: {error}"))
        .ticket
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode search JSON: {error}"))
}
