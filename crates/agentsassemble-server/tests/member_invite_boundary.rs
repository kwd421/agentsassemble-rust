use crate::support::human_invite::{fixture, open_session_socket};
use agentsassemble_domain::{InviteScope, ProviderCatalog};
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use axum::{
    Json, Router,
    body::Bytes,
    http::{HeaderMap, StatusCode, Uri},
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::KeyPair;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

type TestResult = Result<(), Box<dyn std::error::Error>>;
const DEVICE: &str = "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[tokio::test]
async fn member_http_signs_redeem_binds_browser_replays_and_fails_closed() -> TestResult {
    let (store, invite) = fixture(InviteScope::ReadWrite).await;
    store.set_registration_epoch(Some("member-epoch")).await?;
    let identity = store.host_identity().await?;
    let pair = ring::signature::Ed25519KeyPair::from_pkcs8(identity.private_key_pkcs8())
        .map_err(|_| "fixture signing key")?;
    let key = pair.public_key().as_ref().to_vec();
    let server_id = identity.server_id().to_owned();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let central = format!("http://{}", listener.local_addr()?);
    let issuer = central.clone();
    let (calls, mut received) = tokio::sync::mpsc::unbounded_channel();
    let worker = worker_router(key, server_id, issuer, calls);
    let cancellation = CancellationToken::new();
    let worker_shutdown = cancellation.clone();
    let worker_task = tokio::spawn(async move {
        axum::serve(listener, worker)
            .with_graceful_shutdown(worker_shutdown.cancelled_owned())
            .await
    });
    let host = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let base = format!("http://{}", host.local_addr()?);
    let state = AppState::local(
        store.clone(),
        TicketStore::new(Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await?
    .with_central_directory(&central)?;
    let host_shutdown = cancellation.clone();
    let host_task =
        tokio::spawn(async move { serve(host, state, host_shutdown, async { Ok(()) }).await });
    let client = reqwest::Client::new();
    let first = assert_admission(&client, &base, invite.join_code(), &mut received).await?;
    // Real room WebSocket uses the member session through the ordinary session owner.
    let token = first["session_token"].as_str().ok_or("token")?;
    assert_member_socket(&client, &base, token).await;
    assert_failures(
        &store,
        &client,
        &base,
        invite.join_code(),
        token,
        &mut received,
    )
    .await?;
    cancellation.cancel();
    host_task.await??;
    worker_task.await??;
    Ok(())
}

async fn challenge(
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    browser: &str,
) -> Result<Value, reqwest::Error> {
    let response = client
        .post(format!("{base}/api/room-invite/member-challenge"))
        .header("x-device-token", browser)
        .json(&json!({"invite_token":invite}))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::OK);
    response.json().await
}
async fn join(
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    browser: &str,
    challenge: &Value,
    grant: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client.post(format!("{base}/api/room-invite/member-join"))
        .header("x-device-token", browser)
        .json(&json!({"invite_token":invite,"challenge_id":challenge["challenge_id"],"grant_token":grant,"request_id":uuid::Uuid::new_v4().to_string()})).send().await
}

async fn assert_member_socket(client: &reqwest::Client, base: &str, token: &str) {
    let mut socket = open_session_socket(client, base, token).await;
    socket
        .send_json(&json!({"op":"command", "request_id":"member-message",
        "action":"message.send", "payload":{"content":"member message"}}))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!(
        [&first, &second]
            .iter()
            .any(|f| f["op"] == "ack" && f["request_id"] == "member-message")
    );
    assert!([&first, &second].iter().any(|f| {
        f["op"] == "event"
            && f["events"]
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e["content"] == "member message"))
    }));
    socket.close().await;
}

fn worker_router(
    key: Vec<u8>,
    server_id: String,
    issuer: String,
    calls: tokio::sync::mpsc::UnboundedSender<Value>,
) -> Router {
    Router::new().route(
        "/v1/servers/{server}/member-grants/redeem",
        post(move |uri: Uri, headers: HeaderMap, body: Bytes| {
            let calls = calls.clone();
            let key = key.clone();
            let issuer = issuer.clone();
            let server_id = server_id.clone();
            async move {
                assert_eq!(
                    uri.path(),
                    format!("/v1/servers/{server_id}/member-grants/redeem")
                );
                let canonical = format!(
                    "AA-HOST-1\nPOST\n{}\n{}\n{}\n{}",
                    uri.path(),
                    headers["x-aa-host-timestamp"].to_str().unwrap_or_default(),
                    headers["x-aa-host-nonce"].to_str().unwrap_or_default(),
                    URL_SAFE_NO_PAD.encode(Sha256::digest(&body))
                );
                let signature = URL_SAFE_NO_PAD
                    .decode(&headers["x-aa-host-signature"])
                    .unwrap_or_else(|_| panic!("signature"));
                ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, key)
                    .verify(canonical.as_bytes(), &signature)
                    .unwrap_or_else(|_| panic!("route/body signature"));
                let body: Value = serde_json::from_slice(&body).unwrap_or_else(|_| panic!("body"));
                assert_eq!(body["registration_epoch"], "member-epoch");
                calls
                    .send(body.clone())
                    .unwrap_or_else(|_| panic!("capture"));
                let grant = body["grant_token"].as_str().unwrap_or_default();
                if grant == "aamg1.outage" {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"unavailable"})),
                    );
                }
                if grant == "aamg1.invalid" {
                    return (StatusCode::UNAUTHORIZED, Json(json!({"error":"invalid"})));
                }
                if grant == "aamg1.lost" {
                    return (StatusCode::OK, Json(json!({"truncated":"unknown outcome"})));
                }
                let issuer = if grant == "aamg1.foreign" {
                    "https://foreign.example".into()
                } else {
                    issuer
                };
                let name = if grant == "aamg1.second" {
                    "Changed central profile"
                } else {
                    "Member snapshot"
                };
                (
                    StatusCode::OK,
                    Json(json!({"issuer":issuer,"person_id":"person-1","display_name":name})),
                )
            }
        }),
    )
}

async fn assert_admission(
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    received: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let second_device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([1_u8; 32]));
    let first_challenge = challenge(client, base, invite, DEVICE).await?;
    assert_eq!(
        first_challenge["challenge_hash"],
        URL_SAFE_NO_PAD.encode(Sha256::digest(
            first_challenge["challenge_id"]
                .as_str()
                .ok_or("id")?
                .as_bytes()
        ))
    );
    let stolen = join(
        client,
        base,
        invite,
        &second_device,
        &first_challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(stolen.status(), StatusCode::UNAUTHORIZED);
    assert!(received.try_recv().is_err());
    let first = join(
        client,
        base,
        invite,
        DEVICE,
        &first_challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(first.headers()["cache-control"], "private, no-store");
    let first: Value = first.json().await?;
    let call = received.recv().await.ok_or("redeem call")?;
    assert_eq!(call["challenge_hash"], first_challenge["challenge_hash"]);
    assert_eq!(first["display_name"], "Member snapshot");
    assert_eq!(first["stable_identity"], true);
    assert_eq!(
        join(
            client,
            base,
            invite,
            DEVICE,
            &first_challenge,
            "aamg1.first"
        )
        .await?
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(received.try_recv().is_err());
    // The last one-use slot is consumed; a fresh browser can still recover canonical admission.
    let next = challenge(client, base, invite, &second_device).await?;
    let replay = join(client, base, invite, &second_device, &next, "aamg1.second").await?;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay: Value = replay.json().await?;
    assert_eq!(first, replay);
    received.recv().await.ok_or("second redeem")?;
    Ok(first)
}

async fn assert_failures(
    store: &agentsassemble_persistence::SqliteStore,
    client: &reqwest::Client,
    base: &str,
    invite: &str,
    token: &str,
    received: &mut tokio::sync::mpsc::UnboundedReceiver<Value>,
) -> TestResult {
    for grant in [
        "aamg1.outage",
        "aamg1.invalid",
        "aamg1.lost",
        "aamg1.foreign",
    ] {
        let challenge = challenge(client, base, invite, DEVICE).await?;
        let response = join(client, base, invite, DEVICE, &challenge, grant).await?;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let error: Value = response.json().await?;
        assert!(error.get("session_token").is_none());
        received.recv().await.ok_or("failed redeem")?;
        assert_eq!(
            join(client, base, invite, DEVICE, &challenge, grant)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(received.try_recv().is_err());
        // A failed new admission never retires an existing session.
        let fingerprint: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        store.authorize_human_session(&fingerprint).await?;
    }
    Ok(())
}
