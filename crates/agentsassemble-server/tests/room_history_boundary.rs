use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::Client;
use serde_json::json;
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

mod support {
    pub mod human_invite;
    pub mod local_socket;
    pub mod room_socket_peer;
}

use support::{
    human_invite::{canonical_session_token, fixture, join, open_session_socket, start},
    local_socket::connect,
};

const ROOM_ID: &str = "general";

struct RunningServer {
    base_url: String,
    state: AppState,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl RunningServer {
    async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("join room-history runtime: {error}"));
    }
}

#[tokio::test]
async fn local_tcp_history_pages_large_events_without_gaps_or_mutation() {
    let store = local_fixture().await;
    let principal = local_principal();
    for index in 0..50 {
        store
            .execute_message(
                &principal,
                &format!("large-history-{index}"),
                "message.send",
                &json!({"content": format!("{index:02}-{}", "x".repeat(11_990))}),
            )
            .await
            .unwrap_or_else(|error| panic!("commit large history fixture: {error}"));
    }
    let durable_last_seq = store
        .snapshot(ROOM_ID, 0, 1)
        .await
        .unwrap_or_else(|error| panic!("read history high water: {error}"))
        .last_seq;
    assert_eq!(durable_last_seq, 51);

    let server = start_local(store.clone()).await;
    let mut socket = connect(&server.base_url, &server.state, ROOM_ID).await;
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");

    let mut cursor = 0;
    let mut pages = Vec::new();
    for page_index in 0..8 {
        let request_id = format!("history-page-{page_index}");
        socket
            .send_json(&json!({
                "op": "command",
                "request_id": request_id,
                "action": "room.history",
                "payload": {"before_seq": cursor, "limit": 200},
            }))
            .await;
        let ack = socket.receive_json().await;
        assert_eq!(ack["op"], "ack");
        assert_eq!(ack["request_id"], request_id);
        assert_eq!(ack["action"], "room.history");
        assert_eq!(ack["resolution"], "committed");
        assert!(ack.to_string().len() <= 256 * 1024);
        let events = ack["result"]["events"]
            .as_array()
            .unwrap_or_else(|| panic!("history ACK omitted events"));
        assert!(!events.is_empty());
        let sequences = events
            .iter()
            .map(|event| {
                assert_eq!(event["room_id"], ROOM_ID);
                event["seq"]
                    .as_i64()
                    .unwrap_or_else(|| panic!("history event omitted sequence"))
            })
            .collect::<Vec<_>>();
        assert!(sequences.windows(2).all(|pair| pair[0] + 1 == pair[1]));
        assert_eq!(ack["result"]["oldest_seq"], sequences[0]);
        assert_eq!(ack["result"]["last_seq"], durable_last_seq);
        if page_index == 0 {
            assert!(sequences.len() < 51, "oversized page was not frame-fitted");
            assert_eq!(sequences.last().copied(), Some(durable_last_seq));
            assert_eq!(ack["result"]["has_more_before"], true);
        }
        cursor = sequences[0];
        pages.push(sequences);
        if ack["result"]["has_more_before"] == false {
            break;
        }
    }
    let all_sequences = pages.into_iter().rev().flatten().collect::<Vec<_>>();
    assert_eq!(all_sequences, (1..=durable_last_seq).collect::<Vec<_>>());
    assert_eq!(
        store
            .snapshot(ROOM_ID, 0, 1)
            .await
            .unwrap_or_else(|error| panic!("read history after pages: {error}"))
            .last_seq,
        durable_last_seq
    );
    socket.close().await;
    server.stop().await;
}

#[tokio::test]
async fn read_only_human_reads_history_and_revocation_closes_the_socket() {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    store
        .execute_message(
            &local_principal(),
            "read-only-history-fixture",
            "message.send",
            &json!({"content": "read-only history"}),
        )
        .await
        .unwrap_or_else(|error| panic!("commit read-only history fixture: {error}"));
    let server = start(store).await;
    let client = Client::new();
    let admission = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &format!("aad1_{}", URL_SAFE_NO_PAD.encode([0x58; 32])),
        "a23e4567-e89b-12d3-a456-426614174000",
        "History Reader",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admission);
    let mut socket = open_session_socket(&client, &server.base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "read-only-history",
            "action": "room.history",
            "payload": {"limit": 5},
        }))
        .await;
    let ack = socket.receive_json().await;
    assert_eq!(ack["op"], "ack");
    assert_eq!(ack["request_id"], "read-only-history");
    assert!(ack["result"]["events"].as_array().is_some_and(|events| {
        events
            .iter()
            .any(|event| event["content"] == "read-only history")
    }));

    let left = client
        .post(format!("{}/api/room-invite/leave", server.base_url))
        .bearer_auth(session_token)
        .json(&json!({}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("revoke history reader: {error}"));
    assert_eq!(left.status(), reqwest::StatusCode::OK);
    assert!(socket.wait_closed().await);
    server.stop().await;
}

#[tokio::test]
async fn local_tcp_history_work_is_bounded_without_disabling_the_socket() {
    let server = start_local(local_fixture().await).await;
    let mut socket = connect(&server.base_url, &server.state, ROOM_ID).await;
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");

    for index in 0..5 {
        socket
            .send_json(&json!({
                "op": "command",
                "request_id": format!("history-admitted-{index}"),
                "action": "room.history",
                "payload": {"limit": 200},
            }))
            .await;
        assert_eq!(socket.receive_json().await["op"], "ack");
    }
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "history-limited",
            "action": "room.history",
            "payload": {"limit": 200},
        }))
        .await;
    let limited = socket.receive_json().await;
    assert_eq!(limited["op"], "nack");
    assert_eq!(limited["request_id"], "history-limited");
    assert_eq!(limited["resolution"], "rejected");
    assert_eq!(limited["error"]["code"], "history_read_limited");

    socket
        .send_json(&json!({"op": "ping", "nonce": "still-open"}))
        .await;
    let pong = socket.receive_json().await;
    assert_eq!(pong["op"], "pong");
    assert_eq!(pong["nonce"], "still-open");
    socket.close().await;
    server.stop().await;
}

async fn local_fixture() -> SqliteStore {
    let store = SqliteStore::open(&format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    ))
    .await
    .unwrap_or_else(|error| panic!("open room-history store: {error}"));
    store
        .bootstrap_local_authority("6801182b-6a24-4642-a91d-b0491244ec71", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap room-history authority: {error}"));
    store
        .create_room_for_local_operator("b561c876-08cf-47f7-92a3-c7e26ff0294e", ROOM_ID, "General")
        .await
        .unwrap_or_else(|error| panic!("create room-history room: {error}"));
    store
}

fn local_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: ROOM_ID.to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

async fn start_local(store: SqliteStore) -> RunningServer {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind room-history runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read room-history address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        TicketStore::new(Duration::from_secs(30), 16),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build room-history app state: {error}"));
    let server_state = state.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve room-history runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        state: server_state,
        cancellation,
        task,
    }
}
