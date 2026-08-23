use super::*;

#[tokio::test]
async fn create_with_start_is_one_command_with_original_nested_result_and_replay() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create start root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open create/start store: {error}"));
    bootstrap(&store).await;
    let server = start(store, agent_catalog(directory.path())).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start": true,
    });
    send_create(&mut socket, "create-and-start", &payload).await;
    let created = receive_until_ack(&mut socket, 5).await;
    assert_eq!(created["result"]["status"], "created");
    assert_eq!(
        created["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    assert_eq!(created["result"]["participant"]["status"], "detached");
    assert_eq!(
        created["result"]["start"]["agent_session"]["runtime_status"],
        "idle"
    );
    assert_eq!(
        created["result"]["events"][0]["type"],
        "agent_session_created"
    );
    send_create(&mut socket, "create-and-start", &payload).await;
    let replay = receive_json(&mut socket).await;
    assert_eq!(replay["op"], "ack");
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], created["result"]);
    server.stop().await;
}

#[tokio::test]
async fn shutdown_checkpoints_gone_after_aborting_initialization() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create cancellation root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open cancellation store: {error}"));
    bootstrap(&store).await;
    let started_path = directory.path().join("initialization-started");
    let release_path = directory.path().join("release-initialization");
    let fixture = format!(
        "#!/bin/sh\nprintf '%s' \"$$\" > {}\nIFS= read -r initialize\nwhile [ ! -f {} ]; do :; done\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}'\nIFS= read -r initialized\nIFS= read -r thread\nprintf '%s\\n' '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{\"thread\":{{\"id\":\"thread-1\"}}}}}}'\nIFS= read -r forever\n",
        shell_quote(&started_path),
        shell_quote(&release_path),
    );
    let catalog = agent_catalog_with_fixture(directory.path(), fixture.as_bytes());
    let server = start(store, catalog.clone()).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let mut observer = connect(&server.base_url).await;
    subscribe(&mut observer).await;
    let _observer_snapshot = receive_json(&mut observer).await;
    let create_payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": true,
    });
    send_create(&mut socket, "create-cancelled-start", &create_payload).await;
    wait_for_file(&started_path).await;
    let created = receive_json(&mut observer).await;
    assert_eq!(created["op"], "event");
    assert_eq!(created["events"][0]["type"], "agent_session_created");
    let created_sequence = created["events"][0]["seq"]
        .as_i64()
        .unwrap_or_else(|| panic!("creation event has no durable sequence"));

    let mut snapshot_viewer = connect(&server.base_url).await;
    subscribe(&mut snapshot_viewer).await;
    let concurrent_snapshot = receive_json(&mut snapshot_viewer).await;
    assert_eq!(concurrent_snapshot["last_seq"], created_sequence);
    assert_eq!(concurrent_snapshot["events"][0]["seq"], created_sequence);
    assert_eq!(
        concurrent_snapshot["agent_sessions"][0]["session_id"],
        created["events"][0]["participant_id"]
    );
    server.stop_with_interrupted_command().await;
    std::fs::write(&release_path, b"release")
        .unwrap_or_else(|error| panic!("release initialization fixture: {error}"));

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen cancellation store: {error}"));
    let restarted = start(reopened, catalog).await;
    let mut recovered_socket = connect(&restarted.base_url).await;
    subscribe(&mut recovered_socket).await;
    let recovered = receive_json(&mut recovered_socket).await;
    assert_eq!(recovered["agent_sessions"][0]["runtime_status"], "starting");
    send_create(
        &mut recovered_socket,
        "create-cancelled-start",
        &create_payload,
    )
    .await;
    let resumed = receive_command_ack(&mut recovered_socket).await;
    assert_eq!(
        resumed["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    assert_eq!(
        resumed["result"]["start"]["agent_session"]["runtime_status"],
        "idle"
    );
    restarted.stop().await;
}
