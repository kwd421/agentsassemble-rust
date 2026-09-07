use agentsassemble_domain::ParticipantStatus;
use agentsassemble_provider::ProviderRuntimeObservation;

use super::lifecycle_resume_retry::create_stopped_agent;
use super::*;

#[tokio::test]
async fn http_room_close_leaves_cleanup_with_existing_watcher_until_exact_runtime_is_gone()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    bootstrap(&store).await;
    let uid = store.snapshot("general", 0, 20).await?.room.room_uid;
    let server = start(store.clone(), agent_catalog(directory.path())).await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    receive_json(&mut socket).await;
    let id = create_stopped_agent(&mut socket, directory.path(), "create-room-close").await;
    send_command(
        &mut socket,
        "start-room-close",
        "agent.start",
        &json!({"agent_id": id}),
    )
    .await;
    assert_eq!(
        receive_command_ack(&mut socket).await["result"]["agent_session"]["runtime_status"],
        "idle"
    );
    let running = store
        .load_runtime_reconciliation_candidate("general", &id)
        .await?
        .unwrap_or_else(|| panic!("running custody missing"));
    let mut events = server.state.rooms.subscribe("general").await;
    let ticket = server
        .state
        .tickets
        .issue_server_operator(LOCAL_OPERATOR_USER_ID.to_owned())
        .await?
        .ticket;
    let response = Client::new()
        .post(format!("{}/api/rooms/lifecycle", server.base_url))
        .bearer_auth(ticket)
        .json(&json!({
            "room_id": "general", "request_id": "close-running-room", "action": "room.close",
            "payload": {"room_uid": uid},
        }))
        .send()
        .await?;
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let committed: Value = response.json().await?;
    assert_eq!(committed["result"]["cleanup_pending"], true);
    assert_eq!(committed["result"]["room"]["status"], "closed");
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await?;
            if event.event_type == "agent_session_state"
                && event
                    .extra
                    .get("agent_session")
                    .is_some_and(|session| session["runtime_status"] == "stopped")
            {
                return Ok::<(), tokio::sync::broadcast::error::RecvError>(());
            }
        }
    })
    .await??;
    assert!(matches!(
        server.provider_adapter.observe(&running.session).await,
        ProviderRuntimeObservation::Gone
    ));
    assert!(
        store
            .load_room_runtime_cleanup_page(None)
            .await?
            .keys
            .is_empty()
    );
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn canonical_removal_stops_exact_runtime_and_old_replay_preserves_readded_runtime()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    bootstrap(&store).await;
    let server = start(store.clone(), agent_catalog(directory.path())).await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let id = create_stopped_agent(&mut socket, directory.path(), "create-removed").await;
    send_command(
        &mut socket,
        "start-removed",
        "agent.start",
        &json!({"agent_id": id}),
    )
    .await;
    assert_eq!(
        receive_command_ack(&mut socket).await["result"]["agent_session"]["runtime_status"],
        "idle"
    );
    let running = store
        .load_runtime_reconciliation_candidate("general", &id)
        .await?
        .unwrap_or_else(|| panic!("running custody missing"));
    assert!(matches!(
        server.provider_adapter.observe(&running.session).await,
        ProviderRuntimeObservation::Adopted { .. }
    ));

    let removal = json!({"participant_id": id});
    send_command(&mut socket, "kick-runtime", "participant.kick", &removal).await;
    let kicked = receive_command_ack(&mut socket).await;
    assert_eq!(kicked["result"]["participant"]["status"], "kicked");
    assert_eq!(kicked["result"]["event"]["type"], "participant_kicked");
    assert_eq!(
        kicked["result"]["events"][0]["agent_session"]["enabled"],
        false
    );
    assert_eq!(
        store.participant("general", &id).await?.status,
        ParticipantStatus::Kicked
    );
    assert!(
        store
            .load_room_runtime_cleanup_page(None)
            .await?
            .keys
            .is_empty()
    );
    assert!(matches!(
        server.provider_adapter.observe(&running.session).await,
        ProviderRuntimeObservation::Gone
    ));

    send_command(
        &mut socket,
        "readd-removed",
        "agent.readd",
        &json!({"agent_id": id, "start": true}),
    )
    .await;
    assert_eq!(
        receive_command_ack(&mut socket).await["result"]["agent_session"]["runtime_status"],
        "idle"
    );
    let replacement = store
        .load_runtime_reconciliation_candidate("general", &id)
        .await?
        .unwrap_or_else(|| panic!("replacement custody missing"));
    assert_ne!(
        replacement.session.runtime_lease_token,
        running.session.runtime_lease_token
    );
    send_command(&mut socket, "kick-runtime", "participant.kick", &removal).await;
    assert_eq!(receive_command_ack(&mut socket).await["deduplicated"], true);
    assert!(matches!(
        server.provider_adapter.observe(&replacement.session).await,
        ProviderRuntimeObservation::Adopted { .. }
    ));
    send_command(
        &mut socket,
        "export-runtime",
        "participant.export",
        &removal,
    )
    .await;
    assert_eq!(
        receive_command_ack(&mut socket).await["result"]["participant"]["status"],
        "exported"
    );
    assert!(matches!(
        server.provider_adapter.observe(&replacement.session).await,
        ProviderRuntimeObservation::Gone
    ));
    socket.close().await;
    server.stop().await;
    Ok(())
}
