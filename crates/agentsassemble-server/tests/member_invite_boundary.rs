use crate::support::human_invite::{fixture, open_session_socket, persist_invite_in_room};
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

#[path = "member_connect_boundary.rs"]
mod connect;

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
    let worker_cancellation = CancellationToken::new();
    let worker_shutdown = worker_cancellation.clone();
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
    let mut roster = state.rooms.subscribe("general").await;
    let rooms = state.rooms.clone();
    let host_shutdown = cancellation.clone();
    let host_task =
        tokio::spawn(async move { serve(host, state, host_shutdown, async { Ok(()) }).await });
    let client = reqwest::Client::new();
    let first = assert_admission(&client, &base, invite.join_code(), &mut received).await?;
    let created = tokio::time::timeout(Duration::from_secs(2), roster.recv()).await??;
    assert_eq!(created.event_type, "room_created");
    // Both first admission and fresh-device member-join publish to the open host room.
    for _ in 0..2 {
        let event = tokio::time::timeout(Duration::from_secs(2), roster.recv()).await??;
        assert_eq!(event.event_type, "participant_joined");
        assert_eq!(event.participant_id.as_deref(), first["agent_id"].as_str());
        assert_eq!(
            event.extra["participant"]["display_name"],
            "Member snapshot"
        );
    }
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
    assert_durable_replay(&store, &invite, &central, &first).await?;
    let before_connect = store.snapshot("general", 0, 200).await?.last_seq;
    let mut reconnect_roster = rooms.subscribe("general").await;
    connect::verify(&store, &client, &base, invite.join_code(), &mut received).await?;
    let event = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = reconnect_roster.recv().await?;
            if event.seq > before_connect {
                return Ok::<_, tokio::sync::broadcast::error::RecvError>(event);
            }
        }
    })
    .await??;
    assert_eq!(event.event_type, "participant_joined");
    assert_eq!(event.participant_id.as_deref(), first["agent_id"].as_str());
    cancellation.cancel();
    host_task.await??;
    worker_cancellation.cancel();
    worker_task.await??;
    Ok(())
}

#[tokio::test]
async fn member_connect_reuses_one_redeemed_challenge_across_joined_rooms() -> TestResult {
    use agentsassemble_persistence::{
        HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
        MemberAdmission, PreparedHumanAdmission,
    };

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
    let worker_cancellation = CancellationToken::new();
    let worker_shutdown = worker_cancellation.clone();
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
    let cancellation = CancellationToken::new();
    let host_shutdown = cancellation.clone();
    let host_task =
        tokio::spawn(async move { serve(host, state, host_shutdown, async { Ok(()) }).await });
    let client = reqwest::Client::new();

    // Establish the central person with a canonical member admission in "general".
    assert_admission(&client, &base, invite.join_code(), &mut received).await?;
    // Seed a second active room the same central person is joined to, so one
    // redeemed connect challenge has two rooms to reuse.
    store
        .create_room_for_local_operator("9c4b2e11-6a1e-4f3a-9d2b-2b8f2f4c5a01", "second", "Second")
        .await?;
    let second_invite = persist_invite_in_room(
        &store,
        "second",
        InviteScope::ReadWrite,
        1,
        "second-guest",
        "Second Guest",
    )
    .await;
    let now = chrono::Utc::now();
    let seeded = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: *second_invite.join_code_fingerprint(),
        },
        [3; 32],
        &HumanAdmissionInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            meeting_id_assertion: "second".into(),
            display_name: String::new(),
            participant_type: "human".into(),
            owner_display_name: String::new(),
            client_id: "seed-second-client".into(),
            avatar_image_url: String::new(),
        },
    )?
    .with_member(MemberAdmission {
        secure: None,
        projection_id: "AAAAAAAAAAAAAAAAAAAAAA".into(),
        issuer: central.clone(),
        person_id: "person-1".into(),
        display_name: "Member snapshot".into(),
        registration_epoch: "member-epoch".into(),
        challenge_fingerprint: [9; 32],
        challenge_expires_at: now + chrono::Duration::seconds(30),
    });
    let HumanAdmissionDecision::Admitted(_) = store.admit_human(&seeded, now).await? else {
        return Err("seeding the second room member admission must commit".into());
    };

    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([9_u8; 32]));
    let challenge: Value = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let listed = client
        .post(format!("{base}/api/member-connect/rooms?room_select_reuse=1"))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":challenge["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(listed.status(), StatusCode::OK);
    let listed: Value = listed.json().await?;
    assert_eq!(listed["room_select_reuse"], true);
    {
        let mut keys: Vec<&str> = listed
            .as_object()
            .ok_or("rooms object")?
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["room_select_reuse", "rooms"]);
    }
    let mut rooms: Vec<String> = listed["rooms"]
        .as_array()
        .ok_or("rooms")?
        .iter()
        .map(|room| room["room_id"].as_str().unwrap_or_default().to_owned())
        .collect();
    rooms.sort();
    assert_eq!(rooms, vec!["general".to_owned(), "second".to_owned()]);
    // One central grant redeems the whole admission.
    received.recv().await.ok_or("connect redeem")?;
    // Without the opt-in the response stays exactly {"rooms":[...]} so an
    // old strict ["rooms"] client keeps joining on a new host.
    let legacy_challenge: Value = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let legacy = client
        .post(format!("{base}/api/member-connect/rooms"))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":legacy_challenge["challenge_id"],"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(legacy.status(), StatusCode::OK);
    let legacy: Value = legacy.json().await?;
    assert!(legacy.get("room_select_reuse").is_none());
    {
        let mut keys: Vec<&str> = legacy
            .as_object()
            .ok_or("legacy rooms object")?
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["rooms"]);
    }
    received.recv().await.ok_or("legacy connect redeem")?;
    let challenge_id = challenge["challenge_id"].as_str().ok_or("challenge")?;

    let general = select_member_room(&client, &base, &device, challenge_id, "general").await?;
    assert_eq!(general.status(), StatusCode::OK);
    let general: Value = general.json().await?;
    assert_eq!(general["meeting_id"], "general");
    // A second joined room selects over the SAME redeemed challenge.
    let second = select_member_room(&client, &base, &device, challenge_id, "second").await?;
    assert_eq!(second.status(), StatusCode::OK);
    let second: Value = second.json().await?;
    assert_eq!(second["meeting_id"], "second");
    assert_ne!(second["session_token"], general["session_token"]);
    assert!(
        received.try_recv().is_err(),
        "reusing the redeemed challenge must not spend another central grant"
    );
    // Exact retry of a reused room replays its own session.
    let retry = select_member_room(&client, &base, &device, challenge_id, "second").await?;
    assert_eq!(retry.status(), StatusCode::OK);
    assert!(
        retry.json::<Value>().await? == second,
        "exact retry must replay the room's own response"
    );
    // A room the person is not joined to still fails closed.
    let other = select_member_room(&client, &base, &device, challenge_id, "other").await?;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);

    cancellation.cancel();
    host_task.await??;
    worker_cancellation.cancel();
    worker_task.await??;
    Ok(())
}

async fn select_member_room(
    client: &reqwest::Client,
    base: &str,
    device: &str,
    challenge_id: &str,
    room: &str,
) -> Result<reqwest::Response, reqwest::Error> {
    client
        .post(format!("{base}/api/member-connect/select"))
        .header("x-device-token", device)
        .json(&json!({"challenge_id":challenge_id,"room_id":room,"client_id":"connect-client"}))
        .send()
        .await
}

#[tokio::test]
async fn member_connect_advertises_channel_bound_select_only_to_opted_in_clients() -> TestResult {
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
    let worker_cancellation = CancellationToken::new();
    let worker_shutdown = worker_cancellation.clone();
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
    let cancellation = CancellationToken::new();
    let host_shutdown = cancellation.clone();
    let host_task =
        tokio::spawn(async move { serve(host, state, host_shutdown, async { Ok(()) }).await });
    let client = reqwest::Client::new();

    assert_admission(&client, &base, invite.join_code(), &mut received).await?;
    let device = format!("aad1_{}", URL_SAFE_NO_PAD.encode([9_u8; 32]));

    async fn listed_rooms(
        client: &reqwest::Client,
        base: &str,
        device: &str,
        query: &str,
    ) -> Result<Value, Box<dyn std::error::Error>> {
        let challenge: Value = client
            .post(format!("{base}/api/member-connect/challenge"))
            .header("x-device-token", device)
            .json(&json!({}))
            .send()
            .await?
            .json()
            .await?;
        let listed = client
            .post(format!("{base}/api/member-connect/rooms{query}"))
            .header("x-device-token", device)
            .json(&json!({"challenge_id":challenge["challenge_id"],"grant_token":"aamc1.connect"}))
            .send()
            .await?;
        assert_eq!(listed.status(), StatusCode::OK);
        Ok(listed.json().await?)
    }
    let keys_of = |listed: &Value| {
        let mut keys: Vec<String> = listed
            .as_object()
            .ok_or("rooms object")
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort_unstable();
        keys
    };

    // No opt-in: exactly {"rooms"}, so an old strict client keeps working.
    let legacy = listed_rooms(&client, &base, &device, "").await?;
    assert_eq!(keys_of(&legacy), vec!["rooms".to_owned()]);
    received.recv().await.ok_or("legacy redeem")?;
    // Reuse-only opt-in: no channel-bound field for a client that cannot parse it.
    let reuse_only = listed_rooms(&client, &base, &device, "?room_select_reuse=1").await?;
    assert_eq!(
        keys_of(&reuse_only),
        vec!["room_select_reuse".to_owned(), "rooms".to_owned()]
    );
    assert_eq!(reuse_only["room_select_reuse"], true);
    assert!(reuse_only.get("room_select_channel_bound").is_none());
    received.recv().await.ok_or("reuse redeem")?;
    // Channel-bound-only opt-in: the field rides its own pair.
    let bound_only = listed_rooms(&client, &base, &device, "?room_select_channel_bound=1").await?;
    assert_eq!(
        keys_of(&bound_only),
        vec!["room_select_channel_bound".to_owned(), "rooms".to_owned()]
    );
    assert_eq!(bound_only["room_select_channel_bound"], true);
    assert!(bound_only.get("room_select_reuse").is_none());
    received.recv().await.ok_or("bound redeem")?;
    // Both opt-ins: both capabilities advertised.
    let both = listed_rooms(
        &client,
        &base,
        &device,
        "?room_select_reuse=1&room_select_channel_bound=1",
    )
    .await?;
    assert_eq!(
        keys_of(&both),
        vec![
            "room_select_channel_bound".to_owned(),
            "room_select_reuse".to_owned(),
            "rooms".to_owned()
        ]
    );
    received.recv().await.ok_or("both redeem")?;

    // A select carrying the channel-bound pair on a live challenge follows
    // the normal path and spends no further central grant.
    let both_challenge: Value = client
        .post(format!("{base}/api/member-connect/challenge"))
        .header("x-device-token", &device)
        .json(&json!({}))
        .send()
        .await?
        .json()
        .await?;
    let both_challenge_id = both_challenge["challenge_id"].as_str().ok_or("challenge")?;
    let both_redeem = client
        .post(format!(
            "{base}/api/member-connect/rooms?room_select_reuse=1&room_select_channel_bound=1"
        ))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":both_challenge_id,"grant_token":"aamc1.connect"}))
        .send()
        .await?;
    assert_eq!(both_redeem.status(), StatusCode::OK);
    received.recv().await.ok_or("select redeem")?;
    let with_query = client
        .post(format!(
            "{base}/api/member-connect/select?room_select_channel_bound=1"
        ))
        .header("x-device-token", &device)
        .json(&json!({"challenge_id":both_challenge_id,"room_id":"general","client_id":"connect-client"}))
        .send()
        .await?;
    assert_eq!(with_query.status(), StatusCode::OK);
    let plain = select_member_room(&client, &base, &device, both_challenge_id, "general").await?;
    assert_eq!(plain.status(), StatusCode::OK);
    assert!(
        received.try_recv().is_err(),
        "selects on a live channel must not spend another central grant"
    );

    cancellation.cancel();
    host_task.await??;
    worker_cancellation.cancel();
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
    let mut body: Value = response.json().await?;
    body["request_id"] = json!(uuid::Uuid::new_v4().to_string());
    Ok(body)
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
        .json(&json!({"invite_token":invite,"challenge_id":challenge["challenge_id"],"grant_token":grant,"request_id":challenge["request_id"],"client_id":format!("client-{browser}")})).send().await
}

async fn assert_member_socket(client: &reqwest::Client, base: &str, token: &str) {
    let mut socket = open_session_socket(client, base, token).await;
    let request_id = uuid::Uuid::new_v4().to_string();
    socket
        .send_json(&json!({"op":"command", "request_id":request_id,
        "action":"message.send", "payload":{"content":"member message"}}))
        .await;
    let first = socket.receive_json().await;
    let second = socket.receive_json().await;
    assert!(
        [&first, &second]
            .iter()
            .any(|f| f["op"] == "ack" && f["request_id"] == request_id)
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
    let handler = post(move |uri: Uri, headers: HeaderMap, body: Bytes| {
        let calls = calls.clone();
        let key = key.clone();
        let issuer = issuer.clone();
        let server_id = server_id.clone();
        async move {
            assert_eq!(
                uri.path(),
                format!(
                    "/v1/servers/{server_id}/{}",
                    if uri.path().contains("member-connect-grants") {
                        "member-connect-grants/redeem"
                    } else {
                        "member-grants/redeem"
                    }
                )
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
                Json(
                    json!({"projection_id":(if grant == "aamg1.replacement" {"BBBBBBBBBBBBBBBBBBBBBQ"} else {"AAAAAAAAAAAAAAAAAAAAAA"}),"issuer":issuer,"person_id":"person-1","display_name":name}),
                ),
            )
        }
    });
    Router::new()
        .route(
            "/v1/servers/{server}/endpoint",
            axum::routing::put(|| async { Json(json!({"status":"ok"})) })
                .delete(|| async { Json(json!({"status":"ok"})) }),
        )
        .route("/v1/servers/{server}/member-grants/redeem", handler.clone())
        .route("/v1/servers/{server}/member-connect-grants/redeem", handler)
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
    let retry = join(
        client,
        base,
        invite,
        DEVICE,
        &first_challenge,
        "aamg1.first",
    )
    .await?;
    assert_eq!(retry.status(), StatusCode::OK);
    let same_response = retry.json::<Value>().await? == first;
    assert!(same_response, "exact HTTP retry must preserve the response");
    let mut changed = first_challenge.clone();
    changed["request_id"] = json!(uuid::Uuid::new_v4().to_string());
    for (browser, challenge, grant) in [
        (DEVICE, &changed, "aamg1.first"),
        (DEVICE, &first_challenge, "aamg1.changed"),
        (second_device.as_str(), &first_challenge, "aamg1.first"),
    ] {
        assert_eq!(
            join(client, base, invite, browser, challenge, grant)
                .await?
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert!(received.try_recv().is_err());
    // The last one-use slot is consumed; a fresh browser can still recover canonical admission.
    let next = challenge(client, base, invite, &second_device).await?;
    let replay = join(client, base, invite, &second_device, &next, "aamg1.second").await?;
    assert_eq!(replay.status(), StatusCode::OK);
    let replay: Value = replay.json().await?;
    assert_join_correlation(&first, &first_challenge, DEVICE);
    assert_join_correlation(&replay, &next, &second_device);
    assert_ne!(first["request_id"], replay["request_id"]);
    assert_ne!(first["client_id"], replay["client_id"]);
    assert_eq!(first["agent_id"], replay["agent_id"]);
    assert_eq!(first["display_name"], replay["display_name"]);
    let new_session = first["session_token"] != replay["session_token"];
    assert!(new_session, "new device needs a new session");
    received.recv().await.ok_or("second redeem")?;
    // Both device sessions remain usable through the server's HTTP/WS authority owner.
    assert_member_socket(
        client,
        base,
        first["session_token"].as_str().ok_or("first token")?,
    )
    .await;
    Ok(replay)
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

// Correlation requirements of parseRoomInviteJoinResponse: nonempty current request,
// room and client IDs on both first admission and another browser's canonical retry.
fn assert_join_correlation(response: &Value, challenge: &Value, browser: &str) {
    assert_eq!(response["status"], "admitted");
    assert_eq!(response["request_id"], challenge["request_id"]);
    assert_eq!(response["meeting_id"], "general");
    assert_eq!(response["client_id"], format!("client-{browser}"));
    for field in [
        "request_id",
        "client_id",
        "session_token",
        "agent_id",
        "owner_id",
        "display_name",
        "provider_kind",
        "connection_kind",
        "room_label",
        "server_id",
        "authority_lineage_id",
    ] {
        assert!(
            response[field]
                .as_str()
                .is_some_and(|value| !value.trim().is_empty()),
            "{field}"
        );
    }
    assert_eq!(response["participant_type"], "human");
    assert_eq!(response["client_type"], "browser");
    assert_eq!(response["invite_scope"], "room");
    for field in ["expires_at", "room_created_at"] {
        assert!(
            chrono::DateTime::parse_from_rfc3339(response[field].as_str().unwrap_or_default())
                .is_ok()
        );
    }
    for field in ["stable_identity", "operator"] {
        assert!(response[field].is_boolean());
    }
    for field in ["room_topic", "owner_display_name"] {
        assert!(response[field].is_string());
    }
}

async fn assert_durable_replay(
    store: &agentsassemble_persistence::SqliteStore,
    invite: &agentsassemble_server::IssuedHumanInviteCredentials,
    issuer: &str,
    first: &Value,
) -> TestResult {
    use agentsassemble_persistence::{
        HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
        MemberAdmission, PreparedHumanAdmission,
    };
    let now = chrono::Utc::now();
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: *invite.join_code_fingerprint(),
        },
        [2; 32],
        &HumanAdmissionInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            meeting_id_assertion: "general".into(),
            display_name: String::new(),
            participant_type: "human".into(),
            owner_display_name: String::new(),
            client_id: "durable-retry-client".into(),
            avatar_image_url: String::new(),
        },
    )?
    .with_member(MemberAdmission {
        secure: None,
        projection_id: "projection-test".into(),
        issuer: issuer.into(),
        person_id: "person-1".into(),
        display_name: "Ignored".into(),
        registration_epoch: "member-epoch".into(),
        challenge_fingerprint: [7; 32],
        challenge_expires_at: now + chrono::Duration::seconds(30),
    });
    let HumanAdmissionDecision::Admitted(commit) = store.admit_human(&request, now).await? else {
        panic!("canonical admission must replay");
    };
    assert!(!commit.deduplicated());
    assert_eq!(json!(commit.result().agent_id), first["agent_id"]);
    assert_eq!(commit.result().client_id, "durable-retry-client");
    let new_session = first["session_token"] != commit.session_bearer();
    assert!(new_session, "new request needs a new session");
    let HumanAdmissionDecision::Admitted(retry) = store.admit_human(&request, now).await? else {
        panic!("exact request must replay");
    };
    assert!(retry.deduplicated());
    assert_eq!(commit.result(), retry.result());
    let same_bearer = commit.session_bearer() == retry.session_bearer();
    assert!(same_bearer);
    Ok(())
}
