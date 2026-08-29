use std::{io::Cursor, time::Duration};

use agentsassemble_domain::{LOCAL_OPERATOR_USER_ID, ProviderCatalog};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "persona-boundary-host-token-00000000001";

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
            .unwrap_or_else(|_| panic!("persona runtime did not stop"))
            .unwrap_or_else(|error| panic!("join persona runtime: {error}"));
    }
}

#[tokio::test]
async fn local_tcp_import_list_and_thumbnail_use_atomic_private_library() {
    let server = start().await;
    let client = Client::new();
    let encoded = STANDARD.encode(png_card());

    let import_ticket = issue_operator(&server.tickets).await;
    let imported = client
        .post(format!("{}/api/personas/import", server.base_url))
        .bearer_auth(&import_ticket)
        .header("origin", "tauri://localhost")
        .json(&json!({
            "filename": "../Harbor Guide.png",
            "data_base64": encoded
        }))
        .send()
        .await
        .unwrap_or_else(|error| panic!("import persona card: {error}"));
    assert_eq!(imported.status(), StatusCode::OK);
    assert_eq!(imported.headers()["cache-control"], "private, no-store");
    assert_eq!(
        imported.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let imported = json_body(imported).await;
    assert_eq!(imported["persona"]["id"], "Harbor-Guide");
    assert_eq!(imported["persona"]["display_name"], "Harbor Guide");
    assert_eq!(imported["persona"]["ignored_feature_count"], 2);
    assert_eq!(
        imported["persona"]["thumbnail_url"],
        "/api/personas/Harbor-Guide/thumbnail"
    );

    let replay = client
        .get(format!("{}/api/personas", server.base_url))
        .bearer_auth(import_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse import ticket: {error}"));
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);

    let listed = client
        .get(format!("{}/api/personas", server.base_url))
        .bearer_auth(issue_operator(&server.tickets).await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("list persona library: {error}"));
    assert_eq!(listed.status(), StatusCode::OK);
    assert_eq!(listed.headers()["cache-control"], "private, no-store");
    let listed = json_body(listed).await;
    assert_eq!(listed["items"].as_array().map(Vec::len), Some(1));
    assert_eq!(listed["items"][0], imported["persona"]);

    let thumbnail_ticket = issue_operator(&server.tickets).await;
    let thumbnail = client
        .get(format!(
            "{}/api/personas/Harbor-Guide/thumbnail",
            server.base_url
        ))
        .bearer_auth(&thumbnail_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read persona thumbnail: {error}"));
    assert_eq!(thumbnail.status(), StatusCode::OK);
    assert_eq!(thumbnail.headers()["content-type"], "image/png");
    assert_eq!(thumbnail.headers()["cache-control"], "private, no-store");
    assert_eq!(thumbnail.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        thumbnail.headers()["content-disposition"],
        "inline; filename=\"thumbnail.png\""
    );
    assert!(
        thumbnail
            .bytes()
            .await
            .unwrap_or_else(|error| panic!("read thumbnail bytes: {error}"))
            .starts_with(b"\x89PNG\r\n\x1a\n")
    );

    let missing = client
        .get(format!(
            "{}/api/personas/Missing/thumbnail",
            server.base_url
        ))
        .bearer_auth(issue_operator(&server.tickets).await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("read missing persona thumbnail: {error}"));
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
    assert_eq!(json_body(missing).await["code"], "persona_not_found");
    server.stop().await;
}

#[tokio::test]
async fn tcp_boundary_consumes_exact_operator_before_reading_import_body() {
    let server = start().await;
    let client = Client::new();

    let wrong_purpose = server
        .tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue crossed settings ticket: {error}"))
        .ticket;
    let crossed = client
        .post(format!("{}/api/personas/import", server.base_url))
        .bearer_auth(&wrong_purpose)
        .header("content-type", "application/json")
        .body("not-json")
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross persona ticket: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);
    assert!(
        server
            .tickets
            .consume_settings_directory_read(&wrong_purpose)
            .await
            .is_err(),
        "wrong-purpose authority must be consumed"
    );

    let malformed_ticket = issue_operator(&server.tickets).await;
    let malformed = client
        .post(format!("{}/api/personas/import", server.base_url))
        .bearer_auth(&malformed_ticket)
        .header("content-type", "application/json")
        .body("not-json")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject malformed persona request: {error}"));
    assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
    let consumed = client
        .get(format!("{}/api/personas", server.base_url))
        .bearer_auth(malformed_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse malformed request ticket: {error}"));
    assert_eq!(consumed.status(), StatusCode::UNAUTHORIZED);

    let unsupported = client
        .post(format!("{}/api/personas/import", server.base_url))
        .bearer_auth(issue_operator(&server.tickets).await)
        .json(&json!({"filename": "card.txt", "data_base64": "e30="}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject unsupported persona file: {error}"));
    assert_eq!(unsupported.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(unsupported).await["code"],
        "persona_import_invalid"
    );

    let empty = client
        .get(format!("{}/api/personas", server.base_url))
        .bearer_auth(issue_operator(&server.tickets).await)
        .send()
        .await
        .unwrap_or_else(|error| panic!("list untouched library: {error}"));
    assert_eq!(json_body(empty).await["items"], json!([]));
    server.stop().await;
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open persona store: {error}"));
    store
        .bootstrap_local_authority("d56d0114-5bbf-4a33-b52a-b222ad0bdca4", "Persona Operator")
        .await
        .unwrap_or_else(|error| panic!("bootstrap persona authority: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind persona runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read persona runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        tickets.clone(),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate persona host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build persona app state: {error}"));
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve persona runtime: {error}"));
    });
    RunningServer {
        base_url: format!("http://{address}"),
        tickets,
        cancellation,
        task,
    }
}

async fn issue_operator(tickets: &TicketStore) -> String {
    tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue persona operator ticket: {error}"))
        .ticket
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode persona response: {error}"))
}

fn png_card() -> Vec<u8> {
    let card = json!({
        "spec": "chara_card_v3",
        "spec_version": "3.0",
        "data": {
            "name": "Harbor Guide",
            "description": "Keeps watch.",
            "character_book": {"entries": [
                {"keys": ["harbor"], "content": "The bell rings."},
                {"key": ".*", "content": "must stay inert", "use_regex": true}
            ]},
            "extensions": {"risuai": {"customScripts": [{"in": ".*"}]}}
        }
    });
    let mut output = Cursor::new(Vec::new());
    {
        let mut encoder = png::Encoder::new(&mut output, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .add_text_chunk("ccv3".to_owned(), STANDARD.encode(card.to_string()))
            .unwrap_or_else(|error| panic!("add persona card chunk: {error}"));
        let mut writer = encoder
            .write_header()
            .unwrap_or_else(|error| panic!("write persona PNG header: {error}"));
        writer
            .write_image_data(&[0, 0, 0, 255])
            .unwrap_or_else(|error| panic!("write persona PNG pixel: {error}"));
    }
    output.into_inner()
}
