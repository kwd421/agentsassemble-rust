use std::time::Duration;

use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ProviderCatalog,
};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use reqwest::{Client, RequestBuilder, StatusCode};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

mod support {
    pub mod human_invite;
    pub mod room_socket_peer;
}
use support::human_invite::{fixture, join};

const ORIGIN: &str = "https://recovery.example.test";
const PROXY: &str = "recovery-test-proxy-secret-000000001";

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("recovery HTTP fixture: {error:?}"))
}
fn public(request: RequestBuilder) -> RequestBuilder {
    request
        .header("host", "recovery.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", PROXY)
        .header("origin", ORIGIN)
}
fn device(marker: u8) -> String {
    format!("aad1_{}", URL_SAFE_NO_PAD.encode([marker; 32]))
}

struct Server {
    state: AppState,
    _frontend: tempfile::TempDir,
    client: Client,
    base: String,
    human: Value,
    shutdown: CancellationToken,
    running: tokio::task::JoinHandle<Result<(), agentsassemble_server::ServeError>>,
}
impl Server {
    async fn start() -> Self {
        let (store, invitation) = fixture(InviteScope::ReadOnly).await;
        let listener = checked(TcpListener::bind("127.0.0.1:0").await);
        let address = checked(listener.local_addr());
        let state = checked(
            AppState::local(
                store,
                TicketStore::new(Duration::from_secs(30), 4096),
                ProviderCatalogService::fixed(ProviderCatalog::default()),
            )
            .await,
        );
        let frontend = checked(tempfile::tempdir());
        let source = frontend.path().join("source");
        checked(std::fs::create_dir(&source));
        checked(std::fs::write(
            source.join("index.html"),
            "<html><head><script src=\"./assets/recovery.js\"></script></head><body>Recovery entry fixture</body></html>",
        ));
        checked(std::fs::create_dir(source.join("assets")));
        checked(std::fs::write(
            source.join("assets/recovery.js"),
            "export {};",
        ));
        let state = checked(
            state
                .with_frontend(checked(
                    agentsassemble_server::frontend_release::FrontendRelease::materialize(
                        &source,
                        frontend.path(),
                    ),
                ))
                .with_manual_public_ingress(address, ORIGIN, PROXY),
        );
        let shutdown = CancellationToken::new();
        let running = tokio::spawn(serve(listener, state.clone(), shutdown.clone()));
        let base = format!("http://{address}");
        let client = Client::new();
        let human = join(
            &client,
            &base,
            invitation.invite_token(),
            &device(1),
            "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
            "Recoverable human",
            "",
        )
        .await;
        Self {
            state,
            _frontend: frontend,
            client,
            base,
            human,
            shutdown,
            running,
        }
    }
    fn issue(&self, bearer: &str, device: &str) -> RequestBuilder {
        public(
            self.client
                .post(format!("{}/api/identity/recovery-code", self.base)),
        )
        .bearer_auth(bearer)
        .header("x-device-token", device)
        .json(&json!({}))
    }
    fn redeem(&self, code: &Value, marker: u8) -> RequestBuilder {
        public(self.client.post(format!("{}/api/identity/recovery-code/redeem", self.base)))
            .json(&json!({"recovery_code": code, "room_id": "general", "device_token": device(marker), "client_id": "recovery-client"}))
    }
    async fn stop(self) {
        self.shutdown.cancel();
        checked(checked(self.running.await));
    }
}

#[tokio::test]
async fn public_recovery_retires_old_session_preserves_scope_and_retries_exactly() {
    let server = Server::start().await;
    let bearer = checked(server.human["session_token"].as_str().ok_or("session"));
    let issued = checked(server.issue(bearer, &device(1)).send().await);
    assert_eq!(issued.status(), StatusCode::OK);
    assert_eq!(issued.headers()["cache-control"], "private, no-store");
    let issued: Value = checked(issued.json().await);
    let url = checked(url::Url::parse(checked(
        issued["recovery_url"].as_str().ok_or("URL"),
    )));
    assert_eq!(url.origin().ascii_serialization(), ORIGIN);
    assert_eq!(url.path(), "/recover");
    assert_eq!(
        url.query_pairs()
            .find(|(key, _)| key == "room")
            .map(|(_, value)| value.into_owned()),
        Some("general".into())
    );
    assert_eq!(
        url.fragment(),
        Some(
            format!(
                "recovery={}",
                checked(issued["recovery_code"].as_str().ok_or("code"))
            )
            .as_str()
        )
    );
    let mut revoked = server.state.rooms.session_revocations("general").await;
    let recovered = checked(server.redeem(&issued["recovery_code"], 2).send().await);
    assert_eq!(recovered.status(), StatusCode::OK);
    let recovered: Value = checked(recovered.json().await);
    assert_eq!(recovered["status"], "recovered");
    assert_eq!(recovered["agent_id"], server.human["agent_id"]);
    assert_eq!(recovered["invite_scope"], "read_only");
    assert_eq!(recovered["client_id"], "recovery-client");
    let expected: [u8; 32] = Sha256::digest(bearer.as_bytes()).into();
    assert_eq!(
        checked(checked(
            tokio::time::timeout(Duration::from_secs(1), revoked.recv()).await
        )),
        expected
    );
    assert!(
        server
            .state
            .store
            .authorize_human_session(&expected)
            .await
            .is_err()
    );
    let retry = checked(server.redeem(&issued["recovery_code"], 2).send().await);
    assert_eq!(retry.status(), StatusCode::OK);
    let retry: Value = checked(retry.json().await);
    assert_eq!(retry, recovered);
    let foreign = checked(server.redeem(&issued["recovery_code"], 3).send().await);
    assert_eq!(foreign.status(), StatusCode::FORBIDDEN);
    let mut wrong_origin = checked(server.redeem(&recovered["recovery_code"], 3).build());
    wrong_origin.headers_mut().insert(
        "origin",
        reqwest::header::HeaderValue::from_static("https://other.example.test"),
    );
    assert_eq!(
        checked(server.client.execute(wrong_origin).await).status(),
        StatusCode::FORBIDDEN
    );
    server.stop().await;
}

#[tokio::test]
async fn recovery_issue_rejects_wrong_device_and_paired_operator_authority() {
    let server = Server::start().await;
    let bearer = checked(server.human["session_token"].as_str().ok_or("session"));
    let wrong_device = checked(server.issue(bearer, &device(2)).send().await);
    assert_eq!(wrong_device.status(), StatusCode::CONFLICT);
    let manager = checked(
        server
            .state
            .store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await,
    );
    checked(
        server
            .state
            .store
            .create_operator_pairing(&manager, &[4; 32], ORIGIN, Utc::now())
            .await,
    );
    let paired = checked(
        server
            .state
            .store
            .redeem_operator_pairing(
                &[4; 32],
                &Sha256::digest(device(2).as_bytes()).into(),
                ORIGIN,
                Utc::now(),
            )
            .await,
    );
    let forbidden = checked(
        server
            .issue(&paired.session_bearer, &device(2))
            .send()
            .await,
    );
    assert_eq!(forbidden.status(), StatusCode::UNAUTHORIZED);
    let insecure = checked(
        server
            .client
            .post(format!("{}/api/identity/recovery-code", server.base))
            .bearer_auth(bearer)
            .header("x-device-token", device(1))
            .json(&json!({}))
            .send()
            .await,
    );
    assert_eq!(insecure.status(), StatusCode::UNAUTHORIZED);
    server.stop().await;
}

#[tokio::test]
async fn recovery_link_and_assets_have_their_exact_public_entry() {
    let server = Server::start().await;
    for path in ["/recover", "/recover/", "/recover/assets/recovery.js"] {
        let response = checked(
            public(server.client.get(format!("{}{path}", server.base)))
                .send()
                .await,
        );
        assert_eq!(response.status(), StatusCode::OK);
    }
    let private_root = checked(
        public(server.client.get(format!("{}/", server.base)))
            .send()
            .await,
    );
    assert_eq!(private_root.status(), StatusCode::FORBIDDEN);
    server.stop().await;
}

#[tokio::test]
async fn repeated_invalid_recovery_codes_receive_a_bounded_retry_error() {
    let server = Server::start().await;
    for _ in 0..8 {
        let response = checked(server.redeem(&json!("invalid-code"), 2).send().await);
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
    let limited = checked(server.redeem(&json!("invalid-code"), 2).send().await);
    assert_eq!(limited.status(), StatusCode::TOO_MANY_REQUESTS);
    server.stop().await;
}
