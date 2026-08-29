use std::time::Duration;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

mod support {
    pub mod human_invite;
    pub mod subscription_proof;
}

use support::human_invite::{canonical_session_token, fixture, join, open_session_socket, start};

const HOST_TOKEN: &str = "message-attachment-boundary-host-token-0001";
const PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==";

struct LocalServer {
    base_url: String,
    store: SqliteStore,
    tickets: TicketStore,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}

impl LocalServer {
    async fn stop(self) {
        self.cancellation.cancel();
        self.task
            .await
            .unwrap_or_else(|error| panic!("join message-attachment server: {error}"));
    }
}

#[tokio::test]
async fn local_tcp_upload_and_bound_read_use_exact_one_use_authority() {
    let server = start_local().await;
    let client = Client::new();
    assert_crossed_upload_rejected_before_body(&client, &server).await;
    let text = upload_local(&client, &server, "../notes.txt", "text/plain", b"notes").await;
    let png = upload_local(
        &client,
        &server,
        "pixel.png",
        "image/png",
        &STANDARD
            .decode(PNG_BASE64)
            .unwrap_or_else(|error| panic!("decode local PNG: {error}")),
    )
    .await;
    let unbound_ticket = assert_unbound_read_consumed(&client, &server, &text).await;
    bind_local_attachments(&server.store, &text, &png).await;
    assert_consumed_unbound_read(&client, &server, &text, &unbound_ticket).await;
    assert_crossed_read_consumed(&client, &server, &text).await;
    assert_bound_local_reads(&client, &server, &text, &png).await;
    server.stop().await;
}

async fn assert_crossed_upload_rejected_before_body(client: &Client, server: &LocalServer) {
    let crossed = server
        .tickets
        .issue_preferences_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue crossed upload ticket: {error}"))
        .ticket;
    let rejected = client
        .post(format!("{}/api/attachments", server.base_url))
        .bearer_auth(&crossed)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body("x".repeat(12 * 1024 * 1024))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject crossed upload before body: {error}"));
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let crossed_replay = client
        .get(format!(
            "{}/api/room-settings?room_id=general",
            server.base_url
        ))
        .bearer_auth(crossed)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject consumed crossed ticket: {error}"));
    assert_eq!(crossed_replay.status(), StatusCode::UNAUTHORIZED);
}

async fn assert_unbound_read_consumed(
    client: &Client,
    server: &LocalServer,
    text: &Value,
) -> String {
    let unbound_ticket = issue_local_read(&server.tickets, &text["id"]).await;
    let unbound = read_attachment(
        client,
        &server.base_url,
        &text["download_url"],
        &unbound_ticket,
    )
    .await;
    assert_eq!(unbound.status(), StatusCode::NOT_FOUND);
    unbound_ticket
}

async fn bind_local_attachments(store: &SqliteStore, text: &Value, png: &Value) {
    store
        .execute_message(
            &local_principal(),
            "message-attachment-http-bind",
            "message.send",
            &json!({
                "content": "",
                "attachment_ids": [text["id"], png["id"]]
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("bind HTTP message attachments: {error}"));
}

async fn assert_consumed_unbound_read(
    client: &Client,
    server: &LocalServer,
    text: &Value,
    unbound_ticket: &str,
) {
    let consumed_unbound = read_attachment(
        client,
        &server.base_url,
        &text["download_url"],
        unbound_ticket,
    )
    .await;
    assert_eq!(consumed_unbound.status(), StatusCode::UNAUTHORIZED);
}

async fn assert_crossed_read_consumed(client: &Client, server: &LocalServer, text: &Value) {
    let wrong_id = "ma_ffffffffffffffffffffffffffffffff";
    let crossed_read = issue_local_read_id(&server.tickets, wrong_id).await;
    let wrong_target = read_attachment(
        client,
        &server.base_url,
        &text["download_url"],
        &crossed_read,
    )
    .await;
    assert_eq!(wrong_target.status(), StatusCode::UNAUTHORIZED);
    let crossed_replay = read_attachment(
        client,
        &server.base_url,
        &Value::String(format!("/api/attachments/{wrong_id}?download=1")),
        &crossed_read,
    )
    .await;
    assert_eq!(crossed_replay.status(), StatusCode::UNAUTHORIZED);
}

async fn assert_bound_local_reads(
    client: &Client,
    server: &LocalServer,
    text: &Value,
    png: &Value,
) {
    let text_ticket = issue_local_read(&server.tickets, &text["id"]).await;
    let text_response = read_attachment(
        client,
        &server.base_url,
        &Value::String(format!(
            "/api/attachments/{}?view=1",
            text["id"].as_str().unwrap_or_default()
        )),
        &text_ticket,
    )
    .await;
    assert_private_attachment(&text_response, "text/plain", "attachment");
    assert_eq!(
        text_response.bytes().await.unwrap_or_default(),
        b"notes"[..]
    );

    let png_ticket = issue_local_read(&server.tickets, &png["id"]).await;
    let png_response = read_attachment(client, &server.base_url, &png["url"], &png_ticket).await;
    assert_private_attachment(&png_response, "image/png", "inline");
    assert!(
        png_response
            .bytes()
            .await
            .unwrap_or_default()
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );
}

#[tokio::test]
async fn human_session_tcp_upload_send_and_read_preserve_scope() {
    let client = Client::new();
    let (store, credentials) = fixture(InviteScope::ReadWrite).await;
    let server = start(store).await;
    let admitted = join(
        &client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential(0x51),
        "913e4567-e89b-12d3-a456-426614174000",
        "Attachment Writer",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);
    let upload_ticket = issue_session_ticket(
        &client,
        &server.base_url,
        session_token,
        "message-attachment-upload",
    )
    .await;
    let attachment = upload(
        &client,
        &server.base_url,
        &upload_ticket,
        "remote.txt",
        "text/plain",
        b"remote attachment",
    )
    .await;

    let mut socket = open_session_socket(&client, &server.base_url, session_token).await;
    socket
        .send_json(&json!({
            "op": "command",
            "request_id": "remote-message-attachment-bind",
            "action": "message.send",
            "payload": {"content": "remote file", "attachment_ids": [attachment["id"]]}
        }))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!([&first, &second].iter().any(|frame| {
        frame["op"] == "ack" && frame["request_id"] == "remote-message-attachment-bind"
    }));
    assert!(
        [&first, &second].iter().any(|frame| {
            frame["op"] == "event"
                && frame["events"].as_array().is_some_and(|events| {
                    events
                        .iter()
                        .any(|event| event["attachments"][0]["id"] == attachment["id"])
                })
        }),
        "attachment event missing from {first} and {second}"
    );

    let raw_session = read_attachment(
        &client,
        &server.base_url,
        &attachment["download_url"],
        session_token,
    )
    .await;
    assert_eq!(raw_session.status(), StatusCode::UNAUTHORIZED);
    let read_ticket = issue_session_attachment_read(
        &client,
        &server.base_url,
        session_token,
        attachment["id"].as_str().unwrap_or_default(),
    )
    .await;
    let readable = read_attachment(
        &client,
        &server.base_url,
        &attachment["download_url"],
        &read_ticket,
    )
    .await;
    assert_private_attachment(&readable, "text/plain", "attachment");
    assert_eq!(
        readable.bytes().await.unwrap_or_default(),
        b"remote attachment"[..]
    );
    let replay = read_attachment(
        &client,
        &server.base_url,
        &attachment["download_url"],
        &read_ticket,
    )
    .await;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    socket.close().await;
    server.stop().await;

    assert_read_only_session(&client).await;
}

async fn assert_read_only_session(client: &Client) {
    let (store, credentials) = fixture(InviteScope::ReadOnly).await;
    let pending = store
        .store_message_attachment(
            &local_principal(),
            "shared.txt",
            "text/plain",
            b"shared read-only".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store read-only fixture attachment: {error}"));
    store
        .execute_message(
            &local_principal(),
            "read-only-message-attachment-bind",
            "message.send",
            &json!({"content": "shared", "attachment_ids": [pending.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("bind read-only fixture attachment: {error}"));
    let server = start(store).await;
    let admitted = join(
        client,
        &server.base_url,
        credentials.join_code(),
        &browser_credential(0x52),
        "a13e4567-e89b-12d3-a456-426614174000",
        "Attachment Reader",
        "",
    )
    .await;
    let session_token = canonical_session_token(&admitted);

    let denied = client
        .post(format!(
            "{}/api/session-tickets/message-attachment-upload",
            server.base_url
        ))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("deny read-only upload grant: {error}"));
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert_eq!(json_body(denied).await["code"], "permission_denied");

    let read_ticket =
        issue_session_attachment_read(client, &server.base_url, session_token, &pending.id).await;
    let readable = read_attachment(
        client,
        &server.base_url,
        &Value::String(pending.download_url),
        &read_ticket,
    )
    .await;
    assert_eq!(readable.status(), StatusCode::OK);
    assert_eq!(
        readable.bytes().await.unwrap_or_default(),
        b"shared read-only"[..]
    );
    server.stop().await;
}

async fn start_local() -> LocalServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open message-attachment store: {error}"));
    store
        .bootstrap_local_authority("bcfa735d-2029-432e-8b6a-32fcff4c43f3", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap message-attachment authority: {error}"));
    store
        .create_room_for_local_operator(
            "d77b59d0-4fde-4dd3-80c8-bc5e891697aa",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create message-attachment room: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 64);
    let state = AppState::local(
        store.clone(),
        tickets.clone(),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate message-attachment host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build message-attachment app state: {error}"));
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind message-attachment runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read message-attachment address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve message-attachment runtime: {error}"));
    });
    LocalServer {
        base_url: format!("http://{address}"),
        store,
        tickets,
        cancellation,
        task,
    }
}

async fn upload_local(
    client: &Client,
    server: &LocalServer,
    filename: &str,
    content_type: &str,
    content: &[u8],
) -> Value {
    let ticket = server
        .tickets
        .issue_message_attachment_upload(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue local upload ticket: {error}"))
        .ticket;
    upload(
        client,
        &server.base_url,
        &ticket,
        filename,
        content_type,
        content,
    )
    .await
}

async fn upload(
    client: &Client,
    base_url: &str,
    ticket: &str,
    filename: &str,
    content_type: &str,
    content: &[u8],
) -> Value {
    let response = client
        .post(format!("{base_url}/api/attachments"))
        .bearer_auth(ticket)
        .json(&json!({
            "purpose": "room_attachment",
            "filename": filename,
            "content_type": content_type,
            "data_base64": STANDARD.encode(content)
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("upload message attachment: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["attachment"].clone()
}

async fn issue_local_read(tickets: &TicketStore, attachment_id: &Value) -> String {
    issue_local_read_id(tickets, attachment_id.as_str().unwrap_or_default()).await
}

async fn issue_local_read_id(tickets: &TicketStore, attachment_id: &str) -> String {
    tickets
        .issue_bound_message_attachment_read(
            "general".to_owned(),
            LOCAL_OPERATOR_USER_ID.to_owned(),
            LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            attachment_id.to_owned(),
        )
        .await
        .unwrap_or_else(|error| panic!("issue local attachment read: {error}"))
        .ticket
}

async fn issue_session_ticket(
    client: &Client,
    base_url: &str,
    session_token: &str,
    purpose: &str,
) -> String {
    let response = client
        .post(format!("{base_url}/api/session-tickets/{purpose}"))
        .bearer_auth(session_token)
        .send()
        .await
        .unwrap_or_else(|error| panic!("exchange attachment session ticket: {error}"));
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("attachment session ticket is missing"))
        .to_owned()
}

async fn issue_session_attachment_read(
    client: &Client,
    base_url: &str,
    session_token: &str,
    attachment_id: &str,
) -> String {
    issue_session_ticket(
        client,
        base_url,
        session_token,
        &format!("message-attachment/{attachment_id}"),
    )
    .await
}

async fn read_attachment(
    client: &Client,
    base_url: &str,
    path: &Value,
    ticket: &str,
) -> reqwest::Response {
    client
        .get(format!(
            "{base_url}{}",
            path.as_str()
                .unwrap_or_else(|| panic!("attachment path is missing"))
        ))
        .bearer_auth(ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read message attachment: {error}"))
}

fn assert_private_attachment(response: &reqwest::Response, content_type: &str, mode: &str) {
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], content_type);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap_or_default()
            .starts_with(mode)
    );
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode attachment response JSON: {error}"))
}

fn browser_credential(byte: u8) -> String {
    format!(
        "aad1_{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode([byte; 32])
    )
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
