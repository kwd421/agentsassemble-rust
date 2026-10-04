use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicI64, Ordering},
    },
    time::Duration,
};

use agentsassemble_domain::ProviderCatalog;
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::ProviderCatalogService;
use agentsassemble_server::{AppState, TicketStore, serve};
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, Method, StatusCode},
    routing::post as post_route,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{ED25519, UnparsedPublicKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{net::TcpListener, sync::mpsc, task::JoinHandle};
use tokio_util::sync::CancellationToken;

#[path = "central_owner_boundary/connection.rs"]
mod connection;
#[path = "central_owner_boundary/devices.rs"]
mod devices;
#[path = "central_owner_boundary/invitations.rs"]
mod invitations;
#[path = "central_owner_boundary/lifecycle.rs"]
mod lifecycle;
#[path = "central_owner_boundary/lifetimes.rs"]
mod lifetimes;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

const ORIGIN: &str = "https://owner.example.test";
const SECRET: &str = "central-owner-boundary-proxy-secret-0000001";
const TOKEN: &str = "aacg1.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const SECOND_TOKEN: &str = "aacg1.BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA";
const DEVICE: &str = "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[derive(Clone)]
struct WorkerState {
    calls: mpsc::UnboundedSender<WorkerCall>,
    generation: Arc<AtomicI64>,
    reject: Arc<AtomicBool>,
    unavailable: Arc<AtomicBool>,
}

struct WorkerCall {
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Bytes,
}

async fn endpoint(
    State(state): State<WorkerState>,
    Path(server_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Json<Value> {
    let parsed: Value =
        serde_json::from_slice(&body).unwrap_or_else(|error| panic!("endpoint JSON: {error:?}"));
    state.generation.store(
        parsed["generation"]
            .as_i64()
            .unwrap_or_else(|| panic!("generation")),
        Ordering::SeqCst,
    );
    state
        .calls
        .send(WorkerCall {
            path: format!("/v1/servers/{server_id}/endpoint"),
            method,
            headers,
            body,
        })
        .unwrap_or_else(|error| panic!("record endpoint call: {error:?}"));
    Json(json!({"status": "ok"}))
}

async fn owner_connection(
    State(state): State<WorkerState>,
    Path(server_id): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<Value>) {
    let parsed: Value =
        serde_json::from_slice(&body).unwrap_or_else(|error| panic!("redeem JSON: {error:?}"));
    state
        .calls
        .send(WorkerCall {
            path: format!("/v1/servers/{server_id}/connect-grants/redeem"),
            method,
            headers,
            body,
        })
        .unwrap_or_else(|error| panic!("record redemption call: {error:?}"));
    if state.unavailable.load(Ordering::SeqCst) {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"code": "unavailable"}})),
        );
    }
    if state.reject.load(Ordering::SeqCst)
        || !matches!(parsed["grant_token"].as_str(), Some(TOKEN | SECOND_TOKEN))
        || parsed["origin"] != ORIGIN
        || parsed["generation"].as_i64() != Some(state.generation.load(Ordering::SeqCst))
    {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": {"code": "connect_grant_invalid"}})),
        );
    }
    (
        StatusCode::OK,
        Json(json!({
            "status": "authorized", "server_id": server_id,
            "person_id": "person-owner", "device_id": "device-owner",
            "origin": ORIGIN, "generation": parsed["generation"],
            "expires_at": chrono::Utc::now().timestamp() + 300,
        })),
    )
}

fn verify_signed_call(call: &WorkerCall, public_key: &[u8]) {
    let timestamp = call.headers["x-aa-host-timestamp"]
        .to_str()
        .unwrap_or_else(|error| panic!("host timestamp: {error:?}"));
    let nonce = call.headers["x-aa-host-nonce"]
        .to_str()
        .unwrap_or_else(|error| panic!("host nonce: {error:?}"));
    let signature = URL_SAFE_NO_PAD
        .decode(call.headers["x-aa-host-signature"].as_bytes())
        .unwrap_or_else(|error| panic!("host signature encoding: {error:?}"));
    let body_hash = URL_SAFE_NO_PAD.encode(Sha256::digest(&call.body));
    let transcript = format!(
        "AA-HOST-1\n{}\n{}\n{timestamp}\n{nonce}\n{body_hash}",
        call.method, call.path
    );
    UnparsedPublicKey::new(&ED25519, public_key)
        .verify(transcript.as_bytes(), &signature)
        .unwrap_or_else(|error| panic!("host request signature: {error:?}"));
}

async fn next_call(calls: &mut mpsc::UnboundedReceiver<WorkerCall>) -> WorkerCall {
    tokio::time::timeout(Duration::from_secs(5), calls.recv())
        .await
        .unwrap_or_else(|error| panic!("central call timeout: {error:?}"))
        .unwrap_or_else(|| panic!("central call channel closed"))
}

fn entry_body(token: &str, generation: i64) -> Value {
    json!({"grant_token": token, "generation": generation,
        "device": {"device_name": "Chrome · macOS", "browser": "Chrome", "os": "macOS"}})
}

async fn post(
    client: &reqwest::Client,
    address: SocketAddr,
    path: &str,
    body: Value,
    origin: &str,
    device: &str,
) -> reqwest::Response {
    client
        .post(format!("http://{address}{path}"))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", origin)
        .header("x-device-token", device)
        .json(&body)
        .send()
        .await
        .unwrap_or_else(|error| panic!("central owner request: {error:?}"))
}

struct Fixture {
    store: SqliteStore,
    address: SocketAddr,
    room_uid: String,
    session_token: String,
    client: reqwest::Client,
    calls: mpsc::UnboundedReceiver<WorkerCall>,
    worker_state: WorkerState,
    worker_task: JoinHandle<()>,
    cancel: CancellationToken,
    host_task: JoinHandle<()>,
    connections: tokio_util::task::TaskTracker,
    tickets: TicketStore,
}

async fn start_fixture() -> Fixture {
    let worker_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("worker listener: {error:?}"));
    let worker_url = format!(
        "http://{}",
        worker_listener
            .local_addr()
            .unwrap_or_else(|error| panic!("worker address: {error:?}"))
    );
    let (call_tx, calls) = mpsc::unbounded_channel();
    let worker_state = WorkerState {
        calls: call_tx,
        generation: Arc::new(AtomicI64::new(0)),
        reject: Arc::new(AtomicBool::new(false)),
        unavailable: Arc::new(AtomicBool::new(false)),
    };
    let worker = Router::new()
        .route(
            "/v1/servers/{server_id}/endpoint",
            axum::routing::put(endpoint).delete(endpoint),
        )
        .route(
            "/v1/servers/{server_id}/connect-grants/redeem",
            post_route(owner_connection),
        )
        .with_state(worker_state.clone());
    let worker_task: JoinHandle<()> = tokio::spawn(async move {
        axum::serve(worker_listener, worker)
            .await
            .unwrap_or_else(|error| panic!("mock worker: {error:?}"));
    });

    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap_or_else(|error| panic!("host listener: {error:?}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|error| panic!("host address: {error:?}"));
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("host store: {error:?}"));
    store
        .bootstrap_local_authority("518f301c-e3bf-4b1c-82dd-5853bacb837f", "Host")
        .await
        .unwrap_or_else(|error| panic!("host bootstrap: {error:?}"));
    let room_uid = String::new();
    let runtime_state = AppState::local(
        store.clone(),
        // Match the runtime capacity: small stores reserve all grants for private use.
        TicketStore::new(Duration::from_secs(30), 4096),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("host state: {error:?}"))
    .with_manual_public_ingress(address, ORIGIN, SECRET)
    .unwrap_or_else(|error| panic!("public ingress: {error:?}"))
    .with_central_directory(&worker_url)
    .unwrap_or_else(|error| panic!("central directory: {error:?}"));
    let connections = runtime_state.connections.clone();
    let tickets = runtime_state.tickets.clone();
    let cancel = CancellationToken::new();
    let owner_cancel = cancel.clone();
    let host_task = tokio::spawn(async move {
        serve(listener, runtime_state, owner_cancel, async { Ok(()) })
            .await
            .unwrap_or_else(|error| panic!("host serve: {error:?}"));
    });
    let client = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap_or_else(|error| panic!("client: {error:?}"));
    Fixture {
        store,
        address,
        room_uid,
        session_token: String::new(),
        client,
        calls,
        worker_state,
        worker_task,
        cancel,
        host_task,
        connections,
        tickets,
    }
}

async fn verify_empty_workspace(fixture: &mut Fixture, generation: i64) {
    let client = &fixture.client;
    let address = fixture.address;
    let directory_body = json!({"session_token": fixture.session_token, "generation": generation});
    let wrong_origin = post(
        client,
        address,
        "/api/central-owner/directory",
        directory_body.clone(),
        "https://evil.example.test",
        DEVICE,
    )
    .await;
    assert_eq!(wrong_origin.status(), StatusCode::FORBIDDEN);
    let missing_device = post(
        client,
        address,
        "/api/central-owner/directory",
        directory_body.clone(),
        ORIGIN,
        "invalid",
    )
    .await;
    assert_eq!(missing_device.status(), StatusCode::UNAUTHORIZED);
    let directory = post(
        client,
        address,
        "/api/central-owner/directory",
        directory_body.clone(),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(directory.status(), StatusCode::OK);
    let listing: Value = directory
        .json()
        .await
        .unwrap_or_else(|error| panic!("directory JSON: {error:?}"));
    assert_eq!(listing["rooms"], json!([]));
    assert_eq!(listing["profile_revision"], 1);
    let workspace_changes = fixture.store.subscribe_room_directory();
    verify_owner_profile(client, address, &fixture.session_token, Some(generation)).await;
    assert!(
        workspace_changes
            .has_changed()
            .unwrap_or_else(|error| panic!("profile notification: {error}"))
    );
    let created = post(client, address, "/api/central-owner/rooms", json!({
        "session_token": fixture.session_token, "generation": generation,
        "request_id": "20000000-0000-4000-8000-000000000015", "room_id": "general", "label": "General"
    }), ORIGIN, DEVICE).await;
    assert_eq!(created.status(), StatusCode::OK);
    let created: Value = created
        .json()
        .await
        .unwrap_or_else(|error| panic!("first room: {error:?}"));
    created["room"]["room_uid"]
        .as_str()
        .unwrap_or_else(|| panic!("first room UID"))
        .clone_into(&mut fixture.room_uid);
}

async fn verify_routes(fixture: &mut Fixture, generation: i64) {
    verify_empty_workspace(fixture, generation).await;
    let client = &fixture.client;
    let address = fixture.address;
    let room_uid = &fixture.room_uid;
    let stale_generation = post(
        client,
        address,
        "/api/central-owner/directory",
        json!({"session_token": fixture.session_token, "generation": generation - 1}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(stale_generation.status(), StatusCode::UNAUTHORIZED);
    let room = post(
        client,
        address,
        "/api/central-owner/room",
        json!({
            "session_token": fixture.session_token, "generation": generation,
            "room_id": "general", "room_uid": room_uid,
        }),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(room.status(), StatusCode::OK);
    let admission: Value = room
        .json()
        .await
        .unwrap_or_else(|error| panic!("room admission JSON: {error:?}"));
    assert_eq!(admission["meeting_id"], "general");
    assert_eq!(admission["status"], "admitted");
    assert_eq!(admission["central_owner"], true);
    verify_owner_profile(
        client,
        address,
        admission["session_token"]
            .as_str()
            .unwrap_or_else(|| panic!("owner session")),
        None,
    )
    .await;
    invitations::verify(client, address, &admission).await;

    verify_owner_rooms(
        client,
        address,
        generation,
        &fixture.session_token,
        &admission,
    )
    .await;
    lifecycle::verify(fixture, generation).await;
}

async fn verify_owner_rooms(
    client: &reqwest::Client,
    address: SocketAddr,
    generation: i64,
    session_token: &str,
    initial: &Value,
) {
    let create = json!({"session_token": session_token, "generation": generation,
        "request_id": "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaab", "room_id": "second", "label": "Second"});
    let rejected = post(
        client,
        address,
        "/api/central-owner/rooms",
        create.clone(),
        ORIGIN,
        "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let mut room_uid = Value::Null;
    for replay in [false, true] {
        let response = post(
            client,
            address,
            "/api/central-owner/rooms",
            create.clone(),
            ORIGIN,
            DEVICE,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let result: Value = response
            .json()
            .await
            .unwrap_or_else(|error| panic!("create: {error:?}"));
        assert_eq!(result["deduplicated"], replay);
        room_uid = result["room"]["room_uid"].clone();
    }
    let mut conflicting = create.clone();
    conflicting["label"] = json!("Changed");
    let conflict = post(
        client,
        address,
        "/api/central-owner/rooms",
        conflicting,
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    for (room_id, uid, device, expected) in [
        ("second", room_uid, DEVICE, StatusCode::OK),
        (
            "general",
            initial["room_uid"].clone(),
            DEVICE,
            StatusCode::OK,
        ),
        (
            "general",
            initial["room_uid"].clone(),
            "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let response = post(client, address, "/api/central-owner/room",
            json!({"session_token": session_token, "generation": generation, "room_id": room_id, "room_uid": uid}), ORIGIN, device).await;
        assert_eq!(response.status(), expected);
        if expected == StatusCode::OK {
            let session: Value = response
                .json()
                .await
                .unwrap_or_else(|error| panic!("room session: {error:?}"));
            assert_eq!(session["room_uid"], uid);
            assert_eq!(
                session["session_token"] == initial["session_token"],
                room_id == "general"
            );
        }
    }
}

async fn verify_owner_friends(
    client: &reqwest::Client,
    address: SocketAddr,
    session: &str,
    generation: Option<i64>,
) {
    let authorized = |request: reqwest::RequestBuilder, device: &str| {
        let request = request
            .header("host", "owner.example.test")
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", SECRET)
            .header("origin", ORIGIN)
            .header("x-device-token", device)
            .bearer_auth(session);
        if let Some(generation) = generation {
            request.header("x-central-generation", generation)
        } else {
            request
        }
    };
    let friend_url = format!("http://{address}/api/central-owner/friends");
    let friend_id = if generation.is_some() {
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb"
    } else {
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa"
    };
    let draft = json!({"friend_id": friend_id, "expected_revision": 0,
        "details": {"display_name": "Owner contact", "handle": "", "participant_type": "human",
        "provider_kind": "", "connection_kind": "", "agent_id": "", "source_agent_id": "",
        "last_meeting_id": "", "status": "offline", "source": "manual", "last_seen_at": null}});
    let saved = authorized(client.post(&friend_url), DEVICE)
        .json(&draft)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner friend: {error:?}"));
    assert_eq!(saved.status(), StatusCode::OK);
    let saved: Value = saved
        .json()
        .await
        .unwrap_or_else(|error| panic!("friend JSON: {error:?}"));
    assert_eq!(saved["revision"], 1);
    let listed: Value = authorized(client.get(&friend_url), DEVICE)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner friends: {error:?}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("friend list JSON: {error:?}"));
    assert_eq!(listed["friends"], json!([saved]));
    let rejected = authorized(
        client.get(&friend_url),
        "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
    )
    .send()
    .await
    .unwrap_or_else(|error| panic!("friend rejection: {error:?}"));
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    let deleted = authorized(
        client.delete(format!("{friend_url}?friend_id={friend_id}")),
        DEVICE,
    )
    .send()
    .await
    .unwrap_or_else(|error| panic!("friend delete: {error:?}"));
    assert_eq!(deleted.status(), StatusCode::OK);
    let replay = authorized(client.post(&friend_url), DEVICE)
        .json(&draft)
        .send()
        .await
        .unwrap_or_else(|error| panic!("friend recreation: {error:?}"));
    assert_eq!(replay.status(), StatusCode::CONFLICT);
}

async fn verify_owner_profile(
    client: &reqwest::Client,
    address: SocketAddr,
    session: &str,
    generation: Option<i64>,
) {
    let authorized = |request: reqwest::RequestBuilder, device: &str| {
        let request = request
            .header("host", "owner.example.test")
            .header("x-forwarded-proto", "https")
            .header("x-agentsassemble-proxy-token", SECRET)
            .header("origin", ORIGIN)
            .header("x-device-token", device)
            .bearer_auth(session);
        if let Some(generation) = generation {
            request.header("x-central-generation", generation)
        } else {
            request
        }
    };
    verify_owner_friends(client, address, session, generation).await;
    let profile_url = format!("http://{address}/api/user-profile");
    let profile = authorized(client.get(&profile_url), DEVICE)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(profile.status(), StatusCode::OK);
    let profile: Value = profile
        .json()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(profile["profile"]["display_name"], "Host");
    let wrong_device = authorized(
        client.get(&profile_url),
        "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
    )
    .send()
    .await
    .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(wrong_device.status(), StatusCode::UNAUTHORIZED);
    let upload = authorized(client.post(format!("http://{address}/api/attachments")), DEVICE)
        .json(&json!({"purpose": "profile_avatar", "filename": "owner.png", "content_type": "image/png",
            "data_base64": "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg=="}))
        .send().await.unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(upload.status(), StatusCode::OK);
    let upload: Value = upload
        .json()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    let patch = json!({"expected_revision": profile["profile"]["revision"], "display_name": "Web owner",
        "avatar_image_url": upload["attachment"]["url"]});
    let updated = authorized(client.post(&profile_url), DEVICE)
        .json(&patch)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(updated.status(), StatusCode::OK);
    let mut stale_patch = patch.clone();
    stale_patch["display_name"] = json!("Stale writer");
    let conflict = authorized(client.post(&profile_url), DEVICE)
        .json(&stale_patch)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(conflict.status(), StatusCode::CONFLICT);
    let reread: Value = authorized(client.get(&profile_url), DEVICE)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("owner profile boundary: {error:?}"));
    assert_eq!(reread["profile"]["display_name"], "Web owner");
    assert_eq!(
        reread["profile"]["avatar_image_url"],
        upload["attachment"]["url"]
    );
    let restored = authorized(client.post(&profile_url), DEVICE)
        .json(&json!({"expected_revision": reread["profile"]["revision"], "display_name": "Host"}))
        .send()
        .await
        .unwrap_or_else(|error| panic!("restore profile: {error:?}"));
    assert_eq!(restored.status(), StatusCode::OK);
}

#[tokio::test]
async fn central_owner_routes_use_bound_session_and_join_offline_publication() {
    let mut fixture = start_fixture().await;
    let address = fixture.address;
    let online = next_call(&mut fixture.calls).await;
    assert_eq!(online.method, Method::PUT);
    let generation = serde_json::from_slice::<Value>(&online.body)
        .unwrap_or_else(|error| panic!("online body: {error:?}"))["generation"]
        .as_i64()
        .unwrap_or_else(|| panic!("online generation"));
    let info: Value = fixture
        .client
        .get(format!("http://{address}/api/server-info"))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .send()
        .await
        .unwrap_or_else(|error| panic!("server info: {error:?}"))
        .json()
        .await
        .unwrap_or_else(|error| panic!("server info JSON: {error:?}"));
    let public_key = URL_SAFE_NO_PAD
        .decode(
            info["host_public_key_jwk"]["x"]
                .as_str()
                .unwrap_or_else(|| panic!("host key")),
        )
        .unwrap_or_else(|error| panic!("host key encoding: {error:?}"));
    verify_signed_call(&online, &public_key);

    let exchange = post(
        &fixture.client,
        address,
        "/api/central-owner/session",
        entry_body(TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(exchange.status(), StatusCode::OK);
    let exchange: Value = exchange
        .json()
        .await
        .unwrap_or_else(|e| panic!("exchange: {e}"));
    fixture.session_token = exchange["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("session token"))
        .to_owned();
    verify_signed_call(&next_call(&mut fixture.calls).await, &public_key);
    verify_routes(&mut fixture, generation).await;
    assert!(
        fixture.calls.try_recv().is_err(),
        "workspace requests must not redeem entry grants"
    );
    fixture.cancel.cancel();
    fixture
        .host_task
        .await
        .unwrap_or_else(|error| panic!("host join: {error:?}"));
    let offline = next_call(&mut fixture.calls).await;
    assert_eq!(offline.method, Method::DELETE);
    let offline_generation = serde_json::from_slice::<Value>(&offline.body)
        .unwrap_or_else(|error| panic!("offline body: {error:?}"))["generation"]
        .as_i64()
        .unwrap_or_else(|| panic!("offline generation"));
    assert!(offline_generation > generation);
    verify_signed_call(&offline, &public_key);
    fixture.worker_task.abort();
    let _ = fixture.worker_task.await;
}
