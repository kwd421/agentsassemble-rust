use super::*;
use room_socket_peer::RoomSocketPeer;

type Peer = RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

async fn room_socket(fixture: &Fixture, generation: i64) -> Peer {
    let admission = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/room",
        json!({"session_token": fixture.session_token, "generation": generation,
            "room_id": "general", "room_uid": fixture.room_uid}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(admission.status(), StatusCode::OK);
    let admission: Value = admission
        .json()
        .await
        .unwrap_or_else(|e| panic!("room: {e}"));
    let ticket: Value = fixture
        .client
        .post(format!(
            "http://{}/api/session-tickets/socket",
            fixture.address
        ))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", ORIGIN)
        .header("x-device-token", DEVICE)
        .bearer_auth(
            admission["session_token"]
                .as_str()
                .unwrap_or_else(|| panic!("room bearer")),
        )
        .send()
        .await
        .unwrap_or_else(|e| panic!("socket admission: {e}"))
        .error_for_status()
        .unwrap_or_else(|e| panic!("socket denied: {e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("ticket: {e}"));
    let ticket = ticket["ticket"]
        .as_str()
        .unwrap_or_else(|| panic!("ticket bearer"));
    let (wire, _) =
        tokio_tungstenite::connect_async(format!("ws://{}/ws?ticket={ticket}", fixture.address))
            .await
            .unwrap_or_else(|e| panic!("connect: {e}"));
    let mut socket = RoomSocketPeer::new(wire);
    assert_eq!(socket.subscribe(0).await["op"], "subscribed");
    assert_eq!(socket.receive_json().await["op"], "snapshot");
    socket
}

async fn message(socket: &mut Peer, request: &str) {
    socket.send_json(&json!({"op":"command", "request_id":request, "action":"message.send", "payload":{"content":request}})).await;
    let mut ack = false;
    let mut received = false;
    for _ in 0..2 {
        let frame = socket
            .receive_json_with_timeout(Duration::from_secs(5))
            .await;
        if frame["op"] == "ack" {
            assert_eq!(frame["accepted"], true);
            ack = true;
        }
        if frame["op"] == "event" {
            assert_eq!(frame["events"][0]["content"], request);
            received = true;
        }
    }
    assert!(ack && received, "message send/receive failed");
}

async fn stream(fixture: &Fixture, generation: i64) -> reqwest::Response {
    let mut response = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/events",
        json!({"session_token":fixture.session_token, "generation":generation}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let frame = response
        .chunk()
        .await
        .unwrap_or_else(|e| panic!("initial status: {e}"))
        .unwrap_or_else(|| panic!("empty initial status"));
    assert!(String::from_utf8_lossy(&frame).contains("event: owner_session"));
    response
}

async fn status(response: &mut reqwest::Response, expected: &str) {
    tokio::time::timeout(Duration::from_secs(28), async {
        while let Some(frame) = response
            .chunk()
            .await
            .unwrap_or_else(|e| panic!("stream: {e}"))
        {
            if String::from_utf8_lossy(&frame).contains(&format!("\"state\":\"{expected}\"")) {
                return;
            }
        }
        panic!("owner stream ended before {expected}");
    })
    .await
    .unwrap_or_else(|e| panic!("owner state deadline: {e}"));
}

async fn verify(fixture: &mut Fixture, generation: i64) {
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
            json!({"session_token":fixture.session_token, "generation":generation}),
            origin,
            device,
        )
        .await;
        assert_eq!(denied.status(), expected);
    }
    let mut socket = room_socket(fixture, generation).await;
    let mut first = stream(fixture, generation).await;
    let mut second = stream(fixture, generation).await;
    message(&mut socket, "before-renewal").await;
    status(&mut first, "active").await;
    status(&mut second, "active").await;
    let renewal = next_call(&mut fixture.calls).await;
    assert!(renewal.path.ends_with("/owner-connections/renew"));
    assert_eq!(renewal.method, Method::POST);
    assert!(
        fixture.calls.try_recv().is_err(),
        "owner streams must share one renewal"
    );
    message(&mut socket, "after-renewal").await;
    // Dropping one directory does not cancel the remaining socket and stream.
    drop(second);
    fixture.worker_state.reject.store(true, Ordering::SeqCst);
    status(&mut first, "ended").await;
    drop(first);
    let revocation = tokio::time::timeout(Duration::from_secs(2), socket.wait_closed()).await;
    assert!(
        revocation.unwrap_or(false),
        "owner socket survived revocation"
    );
    assert!(next_call(&mut fixture.calls).await.path.ends_with("/renew"));
    let denied = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/directory",
        json!({"session_token":fixture.session_token, "generation":generation}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}

async fn short_fixture(lease_seconds: i64, renew_after: i64) -> (Fixture, i64) {
    let mut fixture = start_fixture().await;
    fixture
        .worker_state
        .renew_after
        .store(renew_after, Ordering::SeqCst);
    fixture
        .worker_state
        .lease_seconds
        .store(lease_seconds, Ordering::SeqCst);
    let published = next_call(&mut fixture.calls).await;
    let generation = serde_json::from_slice::<Value>(&published.body)
        .unwrap_or_else(|e| panic!("published: {e}"))["generation"]
        .as_i64()
        .unwrap_or_else(|| panic!("generation"));
    let exchanged: Value = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        json!({"grant_token":TOKEN, "generation":generation}),
        ORIGIN,
        DEVICE,
    )
    .await
    .error_for_status()
    .unwrap_or_else(|e| panic!("exchange: {e}"))
    .json()
    .await
    .unwrap_or_else(|e| panic!("session: {e}"));
    fixture.session_token = exchanged["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("owner bearer"))
        .to_owned();
    assert!(
        next_call(&mut fixture.calls)
            .await
            .path
            .ends_with("/exchange")
    );
    (fixture, generation)
}

async fn stop(fixture: Fixture) {
    fixture.cancel.cancel();
    fixture
        .host_task
        .await
        .unwrap_or_else(|e| panic!("host join: {e}"));
    fixture.worker_task.abort();
    let _ = fixture.worker_task.await;
}

#[tokio::test]
async fn last_disconnect_cancels_shared_owner_renewal() {
    let (mut fixture, generation) = short_fixture(60, 1).await;
    let mut first = stream(&fixture, generation).await;
    let mut second = stream(&fixture, generation).await;
    let renewal = next_call(&mut fixture.calls).await;
    assert!(renewal.path.ends_with("/renew"));
    status(&mut first, "active").await;
    status(&mut second, "active").await;
    assert!(fixture.calls.try_recv().is_err());
    drop(first);
    drop(second);
    assert!(
        tokio::time::timeout(Duration::from_secs(2), fixture.calls.recv())
            .await
            .is_err(),
        "renewal survived its last connection"
    );
    stop(fixture).await;
}

#[tokio::test]
async fn central_failure_retries_only_inside_the_existing_lease() {
    let (mut fixture, generation) = short_fixture(6, 1).await;
    let mut connected = stream(&fixture, generation).await;
    fixture
        .worker_state
        .unavailable
        .store(true, Ordering::SeqCst);
    assert!(next_call(&mut fixture.calls).await.path.ends_with("/renew"));
    status(&mut connected, "retrying").await;
    status(&mut connected, "ended").await;
    let denied = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/directory",
        json!({"session_token":fixture.session_token,"generation":generation}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    drop(connected);
    stop(fixture).await;
}

#[tokio::test]
async fn renewal_preserves_directory_and_message_socket_and_revocation_closes_both() {
    let (mut fixture, generation) = short_fixture(60, 20).await;
    let created = fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000029",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|e| panic!("room: {e}"));
    fixture.room_uid = created.room.room_uid.to_string();
    verify(&mut fixture, generation).await;
    stop(fixture).await;
}
