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

#[path = "central_owner_boundary/invitations.rs"]
mod invitations;

const ORIGIN: &str = "https://owner.example.test";
const SECRET: &str = "central-owner-boundary-proxy-secret-0000001";
const TOKEN: &str = "aacg1.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
const DEVICE: &str = "aad1_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

#[derive(Clone)]
struct WorkerState {
    calls: mpsc::UnboundedSender<WorkerCall>,
    generation: Arc<AtomicI64>,
    reject: Arc<AtomicBool>,
    expires_at: i64,
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

async fn redeem(
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
    if state.reject.load(Ordering::SeqCst)
        || parsed["grant_token"] != TOKEN
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
            "expires_at": state.expires_at,
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
    client: reqwest::Client,
    calls: mpsc::UnboundedReceiver<WorkerCall>,
    worker_state: WorkerState,
    worker_task: JoinHandle<()>,
    cancel: CancellationToken,
    host_task: JoinHandle<()>,
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
        expires_at: chrono::Utc::now().timestamp() + 300,
    };
    let worker = Router::new()
        .route(
            "/v1/servers/{server_id}/endpoint",
            axum::routing::put(endpoint).delete(endpoint),
        )
        .route(
            "/v1/servers/{server_id}/connect-grants/redeem",
            post_route(redeem),
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
        TicketStore::new(Duration::from_secs(30), 8),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
    )
    .await
    .unwrap_or_else(|error| panic!("host state: {error:?}"))
    .with_manual_public_ingress(address, ORIGIN, SECRET)
    .unwrap_or_else(|error| panic!("public ingress: {error:?}"))
    .with_central_directory(&worker_url)
    .unwrap_or_else(|error| panic!("central directory: {error:?}"));
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
        client,
        calls,
        worker_state,
        worker_task,
        cancel,
        host_task,
    }
}

async fn verify_empty_workspace(fixture: &mut Fixture, generation: i64, public_key: &[u8]) {
    let client = &fixture.client;
    let address = fixture.address;
    let calls = &mut fixture.calls;
    let directory_body = json!({"grant_token": TOKEN, "generation": generation});
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
    verify_signed_call(&next_call(calls).await, public_key);
    let workspace_changes = fixture.store.subscribe_room_directory();
    verify_owner_profile(client, address, TOKEN, Some(generation)).await;
    assert!(
        workspace_changes
            .has_changed()
            .unwrap_or_else(|error| panic!("profile notification: {error}"))
    );
    for _ in 0..12 {
        verify_signed_call(&next_call(calls).await, public_key);
    }
    let created = post(client, address, "/api/central-owner/rooms", json!({
        "grant_token": TOKEN, "generation": generation,
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
    verify_signed_call(&next_call(calls).await, public_key);
}

async fn verify_routes(fixture: &mut Fixture, generation: i64, public_key: &[u8]) {
    verify_empty_workspace(fixture, generation, public_key).await;
    let client = &fixture.client;
    let address = fixture.address;
    let calls = &mut fixture.calls;
    let worker_state = &fixture.worker_state;
    let room_uid = &fixture.room_uid;
    let directory_body = json!({"grant_token": TOKEN, "generation": generation});
    let stale_generation = post(
        client,
        address,
        "/api/central-owner/directory",
        json!({"grant_token": TOKEN, "generation": generation - 1}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(stale_generation.status(), StatusCode::UNAUTHORIZED);
    verify_signed_call(&next_call(calls).await, public_key);
    let room = post(
        client,
        address,
        "/api/central-owner/room",
        json!({
            "grant_token": TOKEN, "generation": generation,
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
    verify_signed_call(&next_call(calls).await, public_key);

    verify_owner_rooms(client, address, generation, calls, public_key, &admission).await;
    worker_state.reject.store(true, Ordering::SeqCst);
    let rejected = post(
        client,
        address,
        "/api/central-owner/directory",
        directory_body,
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
    verify_signed_call(&next_call(calls).await, public_key);
}

async fn verify_owner_rooms(
    client: &reqwest::Client,
    address: SocketAddr,
    generation: i64,
    calls: &mut mpsc::UnboundedReceiver<WorkerCall>,
    public_key: &[u8],
    initial: &Value,
) {
    let create = json!({"grant_token": TOKEN, "generation": generation,
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
    verify_signed_call(&next_call(calls).await, public_key);
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
        verify_signed_call(&next_call(calls).await, public_key);
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
    verify_signed_call(&next_call(calls).await, public_key);
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
            json!({"grant_token": TOKEN, "generation": generation, "room_id": room_id, "room_uid": uid}), ORIGIN, device).await;
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
        verify_signed_call(&next_call(calls).await, public_key);
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
async fn central_owner_routes_redeem_current_grant_and_join_offline_publication() {
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

    verify_routes(&mut fixture, generation, &public_key).await;
    verify_owner_stream(&mut fixture, generation, &public_key).await;
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

async fn verify_owner_stream(fixture: &mut Fixture, generation: i64, public_key: &[u8]) {
    fixture.worker_state.reject.store(false, Ordering::SeqCst);
    let body = json!({"grant_token": TOKEN, "generation": generation});
    for (origin, device, expected) in [
        ("https://evil.example.test", DEVICE, StatusCode::FORBIDDEN),
        (ORIGIN, "invalid", StatusCode::UNAUTHORIZED),
        (
            ORIGIN,
            "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let denied = post(
            &fixture.client,
            fixture.address,
            "/api/central-owner/events",
            body.clone(),
            origin,
            device,
        )
        .await;
        assert_eq!(denied.status(), expected);
        if device.starts_with("aad1_B") {
            verify_signed_call(&next_call(&mut fixture.calls).await, public_key);
        }
    }
    let mut stream = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/events",
        body,
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(stream.status(), StatusCode::OK);
    owner_directory_notice(&mut stream).await;
    for _ in 0..2 {
        verify_signed_call(&next_call(&mut fixture.calls).await, public_key);
    }
    fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000020",
            "stream-change",
            "Committed elsewhere",
        )
        .await
        .unwrap_or_else(|e| panic!("cross-client commit: {e}"));
    owner_directory_notice(&mut stream).await;
    verify_signed_call(&next_call(&mut fixture.calls).await, public_key);
    fixture.worker_state.reject.store(true, Ordering::SeqCst);
    fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000021",
            "revoked-change",
            "Not delivered",
        )
        .await
        .unwrap_or_else(|e| panic!("commit after revocation: {e}"));
    let terminal = tokio::time::timeout(Duration::from_secs(5), stream.chunk())
        .await
        .unwrap_or_else(|e| panic!("revocation deadline: {e}"));
    assert!(
        matches!(terminal, Err(_) | Ok(None)),
        "revoked owner received data"
    );
    verify_signed_call(&next_call(&mut fixture.calls).await, public_key);
}

async fn owner_directory_notice(response: &mut reqwest::Response) {
    let bytes = tokio::time::timeout(Duration::from_secs(5), response.chunk())
        .await
        .unwrap_or_else(|e| panic!("directory deadline: {e}"))
        .unwrap_or_else(|e| panic!("directory frame: {e}"))
        .unwrap_or_else(|| panic!("directory stream ended"));
    assert_eq!(bytes.as_ref(), b"event: directory_changed\ndata: {}\n\n");
}
