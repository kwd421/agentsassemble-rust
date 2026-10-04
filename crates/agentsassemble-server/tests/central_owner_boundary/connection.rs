use super::*;
use room_socket_peer::RoomSocketPeer;

pub(super) type Peer = RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

pub(super) async fn socket_for_session(fixture: &Fixture, bearer: &str) -> Peer {
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
        .bearer_auth(bearer)
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

pub(super) async fn stream(fixture: &Fixture, generation: i64) -> reqwest::Response {
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

pub(super) async fn status(response: &mut reqwest::Response, expected: &str) {
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

pub(super) async fn admitted_fixture() -> (Fixture, i64) {
    let mut fixture = start_fixture().await;
    let published = next_call(&mut fixture.calls).await;
    let generation = serde_json::from_slice::<Value>(&published.body)
        .unwrap_or_else(|e| panic!("published: {e}"))["generation"]
        .as_i64()
        .unwrap_or_else(|| panic!("generation"));
    let exchanged: Value = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(TOKEN, generation),
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
        .unwrap_or_else(|| panic!("bearer"))
        .to_owned();
    assert!(
        next_call(&mut fixture.calls)
            .await
            .path
            .ends_with("/connect-grants/redeem")
    );
    (fixture, generation)
}

pub(super) async fn stop(fixture: Fixture) {
    fixture.cancel.cancel();
    fixture
        .host_task
        .await
        .unwrap_or_else(|e| panic!("host join: {e}"));
    fixture.worker_task.abort();
    let _ = fixture.worker_task.await;
}
