use super::{connection::*, *};

async fn revoke_current_and_check_closed(
    fixture: &Fixture,
    generation: i64,
    stream: &mut reqwest::Response,
    socket: &mut Peer,
) {
    let listed: Value = authed(fixture, generation, "/api/central-owner/sessions", None)
        .await
        .error_for_status()
        .unwrap_or_else(|e| panic!("list: {e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("devices: {e}"));
    assert_eq!(listed["sessions"][0]["current"], true);
    assert_eq!(listed["sessions"][0]["browser"], "Chrome");
    assert_eq!(listed["sessions"][0]["os"], "macOS");
    assert!(listed["sessions"][0]["last_connected_at"].is_i64());
    let revoked = authed(
        fixture,
        generation,
        "/api/central-owner/sessions/revoke",
        Some(json!({"scope":"session","session_id": listed["sessions"][0]["session_id"]})),
    )
    .await;
    assert_eq!(revoked.status(), StatusCode::OK);
    status(stream, "ended").await;
    assert!(
        tokio::time::timeout(Duration::from_secs(2), socket.wait_closed())
            .await
            .unwrap_or(false),
        "idle socket survived revocation"
    );
    assert_eq!(
        authed(fixture, generation, "/api/central-owner/sessions", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn connected_session_survives_five_minutes_and_central_logout_with_zero_owner_calls() {
    let (mut fixture, generation) = admitted_fixture().await;
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
    let mut stream = stream(&fixture, generation).await;
    let mut socket = room_socket(&fixture, generation).await;
    message(&mut socket, "before-central-logout").await;
    fixture.worker_state.reject.store(true, Ordering::SeqCst);
    fixture
        .worker_state
        .unavailable
        .store(true, Ordering::SeqCst);
    for (seconds, request) in [
        (190, "after-three-minutes"),
        (191, "after-five-minutes-and-central-logout"),
    ] {
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(seconds)).await;
        tokio::time::resume();
        message(&mut socket, request).await;
    }
    assert!(
        fixture.calls.try_recv().is_err(),
        "connected owners contacted central"
    );
    // The next independent admission checks central again and exposes its failure.
    let denied = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(SECOND_TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::BAD_GATEWAY);
    assert!(
        next_call(&mut fixture.calls)
            .await
            .path
            .ends_with("/connect-grants/redeem")
    );
    fixture
        .worker_state
        .unavailable
        .store(false, Ordering::SeqCst);
    let denied = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(SECOND_TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    assert!(
        next_call(&mut fixture.calls)
            .await
            .path
            .ends_with("/connect-grants/redeem")
    );
    revoke_current_and_check_closed(&fixture, generation, &mut stream, &mut socket).await;
    drop(stream);
    drop(socket);
    stop(fixture).await;
}

#[tokio::test]
async fn another_admitted_device_revokes_all_including_the_caller_and_old_entry_cannot_revive() {
    let (mut fixture, generation) = admitted_fixture().await;
    let first_token = fixture.session_token.clone();
    let mut first = stream(&fixture, generation).await;
    let exchanged: Value = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(SECOND_TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await
    .error_for_status()
    .unwrap_or_else(|e| panic!("second entry: {e}"))
    .json()
    .await
    .unwrap_or_else(|e| panic!("second token: {e}"));
    next_call(&mut fixture.calls).await;
    fixture.session_token = exchanged["session_token"]
        .as_str()
        .unwrap_or_else(|| panic!("second bearer"))
        .into();
    let mut second = stream(&fixture, generation).await;
    let listed: Value = authed(&fixture, generation, "/api/central-owner/sessions", None)
        .await
        .json()
        .await
        .unwrap_or_else(|e| panic!("devices: {e}"));
    assert_eq!(
        listed["sessions"]
            .as_array()
            .unwrap_or_else(|| panic!("sessions"))
            .len(),
        2
    );
    let current = listed["sessions"]
        .as_array()
        .unwrap_or_else(|| panic!("sessions"))
        .iter()
        .filter(|row| row["current"] == true)
        .count();
    assert_eq!(current, 1);
    assert_eq!(
        authed(
            &fixture,
            generation,
            "/api/central-owner/sessions/revoke",
            Some(json!({"scope":"all"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    status(&mut first, "ended").await;
    status(&mut second, "ended").await;
    fixture.session_token = first_token;
    assert_eq!(
        authed(&fixture, generation, "/api/central-owner/sessions", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let replay = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    drop(first);
    drop(second);
    stop(fixture).await;
}

#[tokio::test]
async fn last_workspace_transport_disconnect_requires_fresh_central_admission() {
    let (mut fixture, generation) = admitted_fixture().await;
    let first = stream(&fixture, generation).await;
    let second = stream(&fixture, generation).await;
    // One dropped transport does not end the workspace while another is retained.
    drop(first);
    assert_eq!(
        authed(&fixture, generation, "/api/central-owner/sessions", None)
            .await
            .status(),
        StatusCode::OK
    );
    drop(second);
    // Task completion is the barrier for the HTTP-body drop and its owned disconnect.
    fixture.connections.close();
    tokio::time::timeout(Duration::from_secs(20), fixture.connections.wait())
        .await
        .unwrap_or_else(|e| panic!("disconnect deadline: {e}"));
    assert_eq!(
        authed(&fixture, generation, "/api/central-owner/sessions", None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let replay = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    assert!(
        next_call(&mut fixture.calls)
            .await
            .path
            .ends_with("/connect-grants/redeem")
    );
    let fresh = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/session",
        entry_body(SECOND_TOKEN, generation),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(fresh.status(), StatusCode::OK);
    stop(fixture).await;
}
