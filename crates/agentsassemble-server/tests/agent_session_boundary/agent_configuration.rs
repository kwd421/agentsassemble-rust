use super::*;

#[tokio::test]
async fn stopped_runtime_configuration_is_revalidated_replayed_and_startable() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create configure root: {error}"));
    let store = SqliteStore::open(&format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("open configure store: {error}"));
    bootstrap(&store).await;
    let server = start(store, agent_catalog(directory.path())).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    send_create(
        &mut socket,
        "create-for-configuration",
        &json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-boundary-1",
            "display_name": "Terra",
            "workspace": directory.path(),
            "model": "gpt-5.6-terra",
            "permission_mode": "meeting_read_only",
            "start_now": false,
        }),
    )
    .await;
    let created = receive_until_ack(&mut socket, 2).await;
    let session_id = created["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("configured fixture session has no id"))
        .to_owned();
    let configure_payload = json!({
        "agent_id": session_id,
        "catalog_revision": "catalog-boundary-1",
        "model": "gpt-5.6-terra",
        "reasoning_effort": "",
        "service_tier": "",
        "variant": "",
        "execution_harness": "builtin",
        "permission_mode": "meeting_read_only",
        "max_output_tokens": "",
    });
    send_command(
        &mut socket,
        "configure-stopped",
        "agent.configure",
        &configure_payload,
    )
    .await;
    let configured = receive_until_ack(&mut socket, 2).await;
    assert_eq!(configured["result"]["status"], "configured");
    assert_eq!(
        configured["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    assert_public_session(&configured["result"]["agent_session"]);

    send_command(
        &mut socket,
        "configure-stopped",
        "agent.configure",
        &configure_payload,
    )
    .await;
    let replay = receive_json(&mut socket).await;
    assert_eq!(replay["op"], "ack");
    assert_eq!(replay["deduplicated"], true);

    send_command(
        &mut socket,
        "start-after-configuration",
        "agent.start",
        &json!({"agent_id": configure_payload["agent_id"]}),
    )
    .await;
    let started = receive_until_ack(&mut socket, 3).await;
    assert_eq!(started["result"]["agent_session"]["runtime_status"], "idle");
    send_command(
        &mut socket,
        "configure-live",
        "agent.configure",
        &configure_payload,
    )
    .await;
    let mut live_error = None;
    for _ in 0..4 {
        let frame = receive_json(&mut socket).await;
        if !frame["error"]["code"].is_null() {
            live_error = Some(frame);
            break;
        }
    }
    assert_eq!(
        live_error.unwrap_or_else(|| panic!("live configuration error was not delivered"))["error"]
            ["code"],
        "runtime_profile_conflict"
    );
    server.stop().await;
}
