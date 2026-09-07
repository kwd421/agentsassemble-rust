use agentsassemble_domain::ParticipantStatus;
use agentsassemble_provider::ProviderRuntimeObservation;

use super::lifecycle_resume_retry::create_stopped_agent;
use super::*;

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
