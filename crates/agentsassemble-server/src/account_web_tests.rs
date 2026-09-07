use crate::{
    AppState, TicketStore,
    google_accounts::tests::{checked, claims, service_fixture, token},
    serve,
};
use agentsassemble_domain::{LOCAL_OPERATOR_USER_ID, ProviderCatalog};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{Client, Method, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use std::{net::SocketAddr, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};
use tokio_util::sync::CancellationToken;

const ORIGIN: &str = "https://account.example.test";
const PROXY: &str = "account-fixture-proxy-secret-00000001";
struct Fixture {
    address: SocketAddr,
    client: Client,
    tickets: TicketStore,
    cancellation: CancellationToken,
    task: JoinHandle<()>,
}
impl Fixture {
    async fn start() -> (Self, jsonwebtoken::EncodingKey) {
        let listener = checked(TcpListener::bind("127.0.0.1:0").await);
        let address = checked(listener.local_addr());
        let store = checked(SqliteStore::open("sqlite::memory:").await);
        checked(
            store
                .bootstrap_local_authority("55555555-5555-4555-8555-555555555555", "Host")
                .await,
        );
        let tickets = TicketStore::new(Duration::from_secs(30), 16);
        let mut state = checked(
            AppState::local(
                store,
                tickets.clone(),
                ProviderCatalogService::fixed(ProviderCatalog::default()),
            )
            .await,
        );
        let (service, key) = service_fixture().await;
        state.google_accounts = service;
        state = checked(state.with_manual_public_ingress(address, ORIGIN, PROXY));
        let cancellation = CancellationToken::new();
        let stop = cancellation.clone();
        let task = tokio::spawn(async move {
            checked(serve(listener, state, stop).await);
        });
        (
            Self {
                address,
                client: checked(Client::builder().timeout(Duration::from_secs(5)).build()),
                tickets,
                cancellation,
                task,
            },
            key,
        )
    }
    fn public(&self, method: Method, path: &str) -> RequestBuilder {
        self.client
            .request(method, format!("http://{}{path}", self.address))
            .header("host", "account.example.test")
            .header("origin", ORIGIN)
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", PROXY)
    }
    async fn stop(self) {
        self.cancellation.cancel();
        checked(self.task.await);
    }
}

async fn body(request: RequestBuilder, expected: StatusCode) -> Value {
    let response = checked(request.send().await);
    assert_eq!(response.status(), expected);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("private, no-store")
    );
    checked(response.json().await)
}

#[tokio::test]
async fn public_account_login_requires_exact_transport_device_proof_and_one_use_challenge() {
    let (fixture, key) = Fixture::start().await;
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([1; 32]));
    let anonymous = body(fixture.public(Method::GET, "/api/account"), StatusCode::OK).await;
    assert!(anonymous["account"].is_null());
    assert_eq!(anonymous["google"]["enabled"], true);
    for request in [
        fixture
            .public(Method::GET, "/api/account")
            .header("x-device-token", "malformed"),
        fixture
            .public(Method::GET, "/api/account")
            .header("x-device-token", &device)
            .bearer_auth("unknown"),
        fixture
            .public(Method::GET, "/api/account")
            .header("authorization", "Bearer one")
            .header("authorization", "Bearer two"),
    ] {
        body(request, StatusCode::UNAUTHORIZED).await;
    }
    let denied = checked(
        fixture
            .public(Method::POST, "/api/account/google/challenge")
            .header("origin", "https://other.example")
            .header("x-device-token", &device)
            .json(&json!({}))
            .send()
            .await,
    );
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    let challenge = body(
        fixture
            .public(Method::POST, "/api/account/google/challenge")
            .header("x-device-token", &device)
            .json(&json!({})),
        StatusCode::OK,
    )
    .await;
    let nonce = checked(challenge["nonce"].as_str().ok_or("nonce"));
    let payload = json!({"credential": token(&claims(nonce), &key), "nonce": nonce});
    body(
        fixture
            .public(Method::POST, "/api/account/google")
            .header(
                "x-device-token",
                format!("aad1_{}", URL_SAFE_NO_PAD.encode([2; 32])),
            )
            .json(&payload),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let connected = body(
        fixture
            .public(Method::POST, "/api/account/google")
            .header("x-device-token", &device)
            .json(&payload),
        StatusCode::OK,
    )
    .await;
    assert_eq!(connected["status"], "connected");
    assert_eq!(connected["account"]["provider"], "google");
    assert!(
        connected["user"]["user_id"]
            .as_str()
            .is_some_and(|id| id != LOCAL_OPERATOR_USER_ID)
    );
    body(
        fixture
            .public(Method::POST, "/api/account/google")
            .header("x-device-token", &device)
            .json(&payload),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let status = body(
        fixture
            .public(Method::GET, "/api/account")
            .header("x-device-token", &device),
        StatusCode::OK,
    )
    .await;
    assert_eq!(status["account"], connected["account"]);
    body(
        fixture
            .public(Method::DELETE, "/api/account/google")
            .header("x-device-token", &device),
        StatusCode::OK,
    )
    .await;
    let status = body(
        fixture
            .public(Method::GET, "/api/account")
            .header("x-device-token", &device),
        StatusCode::OK,
    )
    .await;
    assert!(status["account"].is_null());
    fixture.stop().await;
}

#[tokio::test]
async fn native_account_ticket_is_local_only_and_public_attempt_does_not_consume_it() {
    let (fixture, _) = Fixture::start().await;
    let ticket = checked(
        fixture
            .tickets
            .issue_server_operator(LOCAL_OPERATOR_USER_ID.into())
            .await,
    );
    body(
        fixture
            .public(Method::GET, "/api/account")
            .bearer_auth(&ticket.ticket),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    let url = format!("http://{}/api/account", fixture.address);
    body(
        fixture.client.get(&url).bearer_auth(&ticket.ticket),
        StatusCode::OK,
    )
    .await;
    body(
        fixture.client.get(&url).bearer_auth(&ticket.ticket),
        StatusCode::UNAUTHORIZED,
    )
    .await;
    fixture.stop().await;
}
