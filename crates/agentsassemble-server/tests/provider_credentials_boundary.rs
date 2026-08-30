use std::time::Duration;

use agentsassemble_domain::{LOCAL_OPERATOR_USER_ID, ProviderCatalog};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, HostSecret, TicketStore, serve};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const HOST_TOKEN: &str = "credential-boundary-host-token-0000000001";

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
            .unwrap_or_else(|_| panic!("credential runtime did not stop"))
            .unwrap_or_else(|error| panic!("join credential runtime: {error}"));
    }
}

#[tokio::test]
async fn tcp_boundary_authenticates_before_body_and_keeps_secrets_out_of_errors() {
    let server = start().await;
    let client = Client::new();
    let route = format!("{}/api/provider-credentials/deepseek", server.base_url);

    let wrong_purpose = server
        .tickets
        .issue_settings_directory_read(LOCAL_OPERATOR_USER_ID.to_owned())
        .await
        .unwrap_or_else(|error| panic!("issue crossed settings ticket: {error}"))
        .ticket;
    let crossed = client
        .post(&route)
        .bearer_auth(&wrong_purpose)
        .header("content-type", "application/json")
        .body("not-json")
        .send()
        .await
        .unwrap_or_else(|error| panic!("cross credential ticket: {error}"));
    assert_eq!(crossed.status(), StatusCode::UNAUTHORIZED);
    assert!(
        server
            .tickets
            .consume_settings_directory_read(&wrong_purpose)
            .await
            .is_err(),
        "wrong-purpose authority must be consumed"
    );

    let invalid_secret = "credential-secret-that-must-not-echo";
    let invalid_ticket = issue_operator(&server.tickets).await;
    let invalid = client
        .post(&route)
        .bearer_auth(&invalid_ticket)
        .header("origin", "tauri://localhost")
        .json(&json!({"api_key": invalid_secret, "unexpected": true}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject malformed credential request: {error}"));
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    assert_eq!(invalid.headers()["cache-control"], "private, no-store");
    assert_eq!(
        invalid.headers()["access-control-allow-origin"],
        "tauri://localhost"
    );
    let invalid_body = invalid
        .text()
        .await
        .unwrap_or_else(|error| panic!("read credential rejection: {error}"));
    assert!(!invalid_body.contains(invalid_secret));
    assert_eq!(
        serde_json::from_str::<Value>(&invalid_body)
            .unwrap_or_else(|error| panic!("decode credential rejection: {error}"))["code"],
        "provider_credential_invalid"
    );

    let consumed = client
        .get(&route)
        .bearer_auth(invalid_ticket)
        .send()
        .await
        .unwrap_or_else(|error| panic!("reuse credential ticket: {error}"));
    assert_eq!(consumed.status(), StatusCode::UNAUTHORIZED);

    let invalid_value = client
        .post(&route)
        .bearer_auth(issue_operator(&server.tickets).await)
        .json(&json!({"api_key": "short"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject short credential: {error}"));
    assert_eq!(invalid_value.status(), StatusCode::BAD_REQUEST);
    assert_eq!(
        json_body(invalid_value).await["code"],
        "provider_credential_invalid"
    );

    let crossed_delete = client
        .delete(&route)
        .header("content-type", "application/json")
        .body("not-empty")
        .send()
        .await
        .unwrap_or_else(|error| panic!("reject unauthorized credential delete: {error}"));
    assert_eq!(crossed_delete.status(), StatusCode::UNAUTHORIZED);

    let preflight = client
        .request(Method::OPTIONS, &route)
        .header("origin", "tauri://localhost")
        .header("access-control-request-method", "DELETE")
        .header("access-control-request-headers", "authorization")
        .send()
        .await
        .unwrap_or_else(|error| panic!("preflight credential delete: {error}"));
    assert_eq!(preflight.status(), StatusCode::OK);
    assert!(
        preflight.headers()["access-control-allow-methods"]
            .to_str()
            .is_ok_and(|methods| methods.split(',').any(|method| method == "DELETE"))
    );

    server.stop().await;
}

async fn start() -> RunningServer {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open credential store: {error}"));
    store
        .bootstrap_local_authority(
            "a96a6b5a-22f6-48d3-a262-cea64ab39fb5",
            "Credential Operator",
        )
        .await
        .unwrap_or_else(|error| panic!("bootstrap credential authority: {error}"));
    let tickets = TicketStore::new(Duration::from_secs(30), 32);
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("bind credential runtime: {error}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("read credential runtime address: {error}"));
    let cancellation = CancellationToken::new();
    let server_cancellation = cancellation.clone();
    let state = AppState::local(
        store,
        tickets.clone(),
        HostSecret::new(HOST_TOKEN)
            .unwrap_or_else(|error| panic!("validate credential host secret: {error}")),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("build credential app state: {error}"));
    let task = tokio::spawn(async move {
        serve(listener, state, server_cancellation)
            .await
            .unwrap_or_else(|error| panic!("serve credential runtime: {error}"));
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
        .unwrap_or_else(|error| panic!("issue credential operator ticket: {error}"))
        .ticket
}

async fn json_body(response: reqwest::Response) -> Value {
    response
        .json()
        .await
        .unwrap_or_else(|error| panic!("decode credential response: {error}"))
}
