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
    assert_eq!(
        created["result"]["events"][0]["agent_session"]["runtime_status"],
        "starting"
    );
    assert_eq!(
        created["result"]["events"][0]["agent_session"]["enabled"],
        true
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
    let created_session_id = created["events"][0]["participant_id"]
        .as_str()
        .unwrap_or_else(|| panic!("creation event has no Agent Session identity"))
        .to_owned();

    let mut snapshot_viewer = connect(&server.base_url).await;
    subscribe(&mut snapshot_viewer).await;
    let concurrent_snapshot = receive_json(&mut snapshot_viewer).await;
    assert_eq!(concurrent_snapshot["last_seq"], created_sequence);
    let snapshot_created = concurrent_snapshot["events"]
        .as_array()
        .and_then(|events| {
            events
                .iter()
                .find(|event| event["type"] == "agent_session_created")
        })
        .unwrap_or_else(|| panic!("snapshot omitted the durable agent creation event"));
    assert_eq!(snapshot_created["seq"], created_sequence);
    assert_eq!(
        snapshot_created["agent_session"],
        concurrent_snapshot["agent_sessions"][0]
    );
    assert_eq!(
        snapshot_created["agent_session"]["runtime_status"],
        "starting"
    );
    assert_eq!(snapshot_created["agent_session"]["enabled"], true);
    assert_eq!(
        concurrent_snapshot["agent_sessions"][0]["session_id"],
        created["events"][0]["participant_id"]
    );
    server.stop_with_interrupted_command().await;
    std::fs::write(&release_path, b"release")
        .unwrap_or_else(|error| panic!("release initialization fixture: {error}"));

    verify_restarted_create_start_recovery(
        &database_url,
        catalog,
        &create_payload,
        &created_session_id,
    )
    .await;
}

#[tokio::test]
async fn same_sidecar_recovers_unconfirmed_start_after_browser_identity_is_lost() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create reconnect root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open reconnect store: {error}"));
    bootstrap(&store).await;
    let recovery_store = store.clone();
    let server = start(store, agent_catalog(directory.path())).await;
    let mut first_socket = connect(&server.base_url).await;
    subscribe(&mut first_socket).await;
    let _snapshot = receive_json(&mut first_socket).await;
    let create_payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": false,
    });
    send_create(&mut first_socket, "create-for-reconnect", &create_payload).await;
    let created = receive_command_ack(&mut first_socket).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"))
        .to_owned();
    let payload = json!({"agent_id": session_id});
    let principal = local_principal();
    let agentsassemble_persistence::AgentStartPlan::Start(effect) = recovery_store
        .prepare_agent_start(&principal, "lost-browser-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare interrupted start: {error}"))
    else {
        panic!("stopped Agent Session must prepare a start effect");
    };
    let reservation = server
        .provider_adapter
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve interrupted start: {error}"));
    recovery_store
        .authorize_agent_start_effect(
            &principal,
            "lost-browser-start",
            &payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize interrupted start: {error}"));
    recovery_store
        .mark_agent_start_unconfirmed(
            &principal,
            &session_id,
            &effect.operation_id,
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            "runtime_start_unconfirmed",
            "provider effect boundary was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark interrupted start unconfirmed: {error}"));
    first_socket.close().await;

    wait_for_recovered_rejection(&recovery_store, &principal, "lost-browser-start", &payload).await;
    let mut returning_socket = connect(&server.base_url).await;
    subscribe(&mut returning_socket).await;
    let recovered = receive_json(&mut returning_socket).await;
    assert_eq!(
        recovered["agent_sessions"][0]["last_error_code"],
        "runtime_start_recovered_gone"
    );
    send_command(
        &mut returning_socket,
        "new-start-after-browser-return",
        "agent.start",
        &payload,
    )
    .await;
    let started = receive_command_ack(&mut returning_socket).await;
    assert_eq!(started["result"]["agent_session"]["runtime_status"], "idle");
    server.stop().await;
}

#[tokio::test]
async fn same_sidecar_quiesces_exact_running_runtime_after_browser_identity_is_lost() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create running recovery root: {error}"));
    let database_url = format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    );
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open running recovery store: {error}"));
    bootstrap(&store).await;
    let recovery_store = store.clone();
    let server = start(store, agent_catalog(directory.path())).await;
    let mut first_socket = connect(&server.base_url).await;
    subscribe(&mut first_socket).await;
    let _snapshot = receive_json(&mut first_socket).await;
    let create_payload = json!({
        "provider_id": "codex",
        "catalog_revision": "catalog-boundary-1",
        "display_name": "Terra",
        "workspace": directory.path(),
        "model": "gpt-5.6-terra",
        "permission_mode": "meeting_read_only",
        "start_now": true,
    });
    send_create(
        &mut first_socket,
        "create-running-for-owner-loss",
        &create_payload,
    )
    .await;
    let created = receive_until_ack(&mut first_socket, 5).await;
    let session_id = created["result"]["start"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("started session has no id"))
        .to_owned();
    let payload = json!({"agent_id": session_id});
    let principal = local_principal();
    stage_unconfirmed_running_reuse(
        &recovery_store,
        &server.provider_adapter,
        &principal,
        &session_id,
        &payload,
    )
    .await;
    first_socket.close().await;

    wait_for_recovered_rejection(
        &recovery_store,
        &principal,
        "lost-browser-running-start",
        &payload,
    )
    .await;
    let recovered = recovery_store
        .load_runtime_reconciliation_candidate("general", &session_id)
        .await
        .unwrap_or_else(|error| panic!("reload recovered running session: {error}"));
    assert!(
        recovered.is_none(),
        "recovered running runtime retained private lifecycle authority"
    );

    let mut returning_socket = connect(&server.base_url).await;
    subscribe(&mut returning_socket).await;
    let snapshot = receive_json(&mut returning_socket).await;
    assert_eq!(
        snapshot["agent_sessions"][0]["last_error_code"],
        "runtime_start_recovered_gone"
    );
    send_command(
        &mut returning_socket,
        "new-start-after-running-owner-loss",
        "agent.start",
        &payload,
    )
    .await;
    let restarted = receive_command_ack(&mut returning_socket).await;
    assert_eq!(
        restarted["result"]["agent_session"]["runtime_status"],
        "idle"
    );
    assert_eq!(restarted["result"]["runtime_reused"], false);
    server.stop().await;
}

async fn stage_unconfirmed_running_reuse(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    principal: &AuthenticatedPrincipal,
    session_id: &str,
    payload: &Value,
) {
    let agentsassemble_persistence::AgentStartPlan::Start(effect) = store
        .prepare_agent_start(principal, "lost-browser-running-start", payload)
        .await
        .unwrap_or_else(|error| panic!("prepare running reuse: {error}"))
    else {
        panic!("running Agent Session must prepare an exact reuse effect");
    };
    let reservation = provider_adapter
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve running reuse: {error}"));
    store
        .authorize_agent_start_effect(
            principal,
            "lost-browser-running-start",
            payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize running reuse: {error}"));
    let authorized = store
        .load_runtime_reconciliation_candidate("general", session_id)
        .await
        .unwrap_or_else(|error| panic!("load authorized running reuse: {error}"))
        .unwrap_or_else(|| panic!("authorized running reuse had no recovery candidate"));
    let started = provider_adapter
        .start_reserved(&authorized.session)
        .await
        .unwrap_or_else(|error| panic!("execute running reuse: {error}"));
    store
        .mark_agent_start_unconfirmed(
            principal,
            session_id,
            &effect.operation_id,
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            "runtime_start_unconfirmed",
            "provider effect boundary was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark running reuse unconfirmed: {error}"));
}

async fn wait_for_recovered_rejection(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
) {
    let recovered = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match store
                .prepare_agent_start(principal, request_id, payload)
                .await
            {
                Err(agentsassemble_persistence::PersistenceError::StoredCommandRejected {
                    code,
                    ..
                }) if code == "runtime_start_recovered_gone" => return,
                Err(agentsassemble_persistence::PersistenceError::CommandUnresolved { .. }) => {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
                outcome => panic!("unexpected recovery outcome: {outcome:?}"),
            }
        }
    })
    .await;
    assert!(recovered.is_ok(), "dynamic lifecycle recovery timed out");
}

async fn verify_restarted_create_start_recovery(
    database_url: &str,
    catalog: ProviderCatalog,
    create_payload: &Value,
    created_session_id: &str,
) {
    let reopened = SqliteStore::open(database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen cancellation store: {error}"));
    let restarted = start(reopened, catalog).await;
    let mut recovered_socket = connect(&restarted.base_url).await;
    subscribe(&mut recovered_socket).await;
    let recovered = receive_json(&mut recovered_socket).await;
    assert_eq!(
        recovered["agent_sessions"][0]["session_id"],
        created_session_id
    );
    assert_eq!(recovered["agent_sessions"][0]["runtime_status"], "error");
    assert_eq!(recovered["agent_sessions"][0]["enabled"], false);
    assert_eq!(
        recovered["agent_sessions"][0]["last_error_code"],
        "runtime_start_recovered_gone"
    );
    send_create(
        &mut recovered_socket,
        "create-cancelled-start",
        create_payload,
    )
    .await;
    let old_request = receive_json(&mut recovered_socket).await;
    assert_eq!(old_request["error"]["code"], "runtime_start_recovered_gone");
    send_command(
        &mut recovered_socket,
        "start-after-recovered-create",
        "agent.start",
        &json!({"agent_id": created_session_id}),
    )
    .await;
    let resumed = receive_until_ack(&mut recovered_socket, 4).await;
    assert_eq!(resumed["result"]["agent_session"]["runtime_status"], "idle");
    restarted.stop().await;
}
