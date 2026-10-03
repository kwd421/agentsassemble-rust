use super::*;

async fn change(
    fixture: &mut Fixture,
    generation: i64,
    key: &[u8],
    body: &Value,
    device: &str,
) -> reqwest::Response {
    let response = fixture
        .client
        .post(format!(
            "http://{}/api/room-session/lifecycle",
            fixture.address
        ))
        .header("host", "owner.example.test")
        .header("x-forwarded-proto", "https")
        .header("x-agentsassemble-proxy-token", SECRET)
        .header("origin", ORIGIN)
        .bearer_auth(TOKEN)
        .header("x-device-token", device)
        .header("x-central-generation", generation)
        .json(body)
        .send()
        .await
        .unwrap_or_else(|error| panic!("owner lifecycle: {error}"));
    verify_signed_call(&next_call(&mut fixture.calls).await, key);
    response
}

pub(super) async fn verify(fixture: &mut Fixture, generation: i64, key: &[u8]) {
    let authority = fixture
        .store
        .local_bootstrap_status()
        .await
        .unwrap_or_else(|e| panic!("bootstrap: {e}"));
    let created = fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000025",
            "lifecycle-proof",
            "Proof",
        )
        .await
        .unwrap_or_else(|e| panic!("create: {e}"));
    let mut body = json!({"server_id": authority.server_id, "authority_lineage_id": authority.authority_lineage_id,
        "room_id": "lifecycle-proof", "request_id": "owner-archive", "action": "room.archive",
        "payload": {"room_uid": created.room.room_uid, "archived": true}});
    let denied = change(
        fixture,
        generation,
        key,
        &body,
        "aad1_BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBA",
    )
    .await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let denied = change(fixture, generation - 1, key, &body, DEVICE).await;
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    for (request, archived) in [("owner-archive", true), ("owner-restore", false)] {
        body["request_id"] = json!(request);
        body["payload"]["archived"] = json!(archived);
        let response = change(fixture, generation, key, &body, DEVICE).await;
        assert_eq!(response.status(), StatusCode::OK);
        let result: Value = response
            .json()
            .await
            .unwrap_or_else(|e| panic!("lifecycle receipt: {e}"));
        assert_eq!(result["resolution"], "committed");
        assert_eq!(
            result["result"]["room"]["status"],
            if archived { "archived" } else { "active" }
        );
        if archived {
            verify_retired_admission(
                fixture,
                generation,
                key,
                created.room.room_uid,
                "room_inactive",
            )
            .await;
        }
    }
    verify_terminal_replay(fixture, generation, key, body, created.room.room_uid).await;
}

async fn verify_terminal_replay(
    fixture: &mut Fixture,
    generation: i64,
    key: &[u8],
    mut body: Value,
    original_uid: uuid::Uuid,
) {
    body["action"] = json!("room.delete");
    body["request_id"] = json!("owner-delete");
    body["payload"] = json!({"room_uid": original_uid, "confirmation_name": "Proof"});
    let mut changes = fixture.store.subscribe_room_directory();
    let pending = change(fixture, generation, key, &body, DEVICE).await;
    assert_eq!(pending.status(), StatusCode::SERVICE_UNAVAILABLE);
    let pending: Value = pending
        .json()
        .await
        .unwrap_or_else(|e| panic!("pending: {e}"));
    assert_eq!(pending["resolution"], "unresolved");
    assert_eq!(pending["code"], "room_deletion_pending");
    tokio::time::timeout(Duration::from_secs(10), async {
        while fixture
            .store
            .room_exists("lifecycle-proof")
            .await
            .unwrap_or_else(|e| panic!("room: {e}"))
        {
            changes
                .changed()
                .await
                .unwrap_or_else(|e| panic!("directory: {e}"));
        }
    })
    .await
    .unwrap_or_else(|e| panic!("terminal deletion: {e}"));
    verify_retired_admission(fixture, generation, key, original_uid, "room_missing").await;
    let completed = change(fixture, generation, key, &body, DEVICE).await;
    assert_eq!(completed.status(), StatusCode::OK);
    let completed: Value = completed
        .json()
        .await
        .unwrap_or_else(|e| panic!("terminal receipt: {e}"));
    assert_eq!(completed["result"]["deleted"], true);
    assert_eq!(completed["deduplicated"], true);
    let new = fixture
        .store
        .create_room_for_local_operator(
            "10000000-0000-4000-8000-000000000026",
            "lifecycle-proof",
            "Proof",
        )
        .await
        .unwrap_or_else(|e| panic!("recreate: {e}"));
    assert_ne!(new.room.room_uid, original_uid);
    let replay = change(fixture, generation, key, &body, DEVICE).await;
    assert_eq!(replay.status(), StatusCode::OK);
    assert_eq!(
        replay
            .json::<Value>()
            .await
            .unwrap_or_else(|e| panic!("replay: {e}")),
        completed
    );
    assert_eq!(
        fixture
            .store
            .snapshot("lifecycle-proof", 0, 20)
            .await
            .unwrap_or_else(|e| panic!("replacement: {e}"))
            .room
            .room_uid,
        new.room.room_uid
    );
    fixture.worker_state.reject.store(true, Ordering::SeqCst);
    let revoked = change(fixture, generation, key, &body, DEVICE).await;
    assert_eq!(revoked.status(), StatusCode::UNAUTHORIZED);
    fixture.worker_state.reject.store(false, Ordering::SeqCst);
}

async fn verify_retired_admission(
    fixture: &mut Fixture,
    generation: i64,
    key: &[u8],
    uid: uuid::Uuid,
    code: &str,
) {
    let response = post(
        &fixture.client,
        fixture.address,
        "/api/central-owner/room",
        json!({"grant_token": TOKEN, "generation": generation,
            "room_id": "lifecycle-proof", "room_uid": uid}),
        ORIGIN,
        DEVICE,
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let result: Value = response
        .json()
        .await
        .unwrap_or_else(|e| panic!("retired admission: {e}"));
    assert_eq!(result["error"]["code"], code);
    verify_signed_call(&next_call(&mut fixture.calls).await, key);
}
