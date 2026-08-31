use agentsassemble_persistence::AgentStartPlan;

use super::*;

#[tokio::test]
async fn current_generation_resume_retry_reaches_its_lifecycle_owner() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create resume retry root: {error}"));
    let database_url = database_url(directory.path());
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open resume retry store: {error}"));
    bootstrap(&store).await;
    let staging_store = store.clone();
    let server = start(store, agent_catalog(directory.path())).await;
    let mut socket = connect(&server.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let session_id =
        create_stopped_agent(&mut socket, directory.path(), "create-current-resume-retry").await;
    let payload = json!({"agent_id": session_id});

    assert!(matches!(
        staging_store
            .prepare_agent_resume(&local_principal(), "current-resume-retry", &payload)
            .await
            .unwrap_or_else(|error| panic!("stage current resume: {error}")),
        AgentStartPlan::Start(_)
    ));
    send_command(
        &mut socket,
        "current-resume-retry",
        "agent.resume",
        &payload,
    )
    .await;
    let resumed = receive_command_ack(&mut socket).await;
    assert_eq!(resumed["result"]["agent_session"]["runtime_status"], "idle");
    send_command(
        &mut socket,
        "current-resume-retry",
        "agent.resume",
        &payload,
    )
    .await;
    assert_eq!(receive_command_ack(&mut socket).await["deduplicated"], true);

    send_command(
        &mut socket,
        "stop-current-resume-retry",
        "agent.stop",
        &payload,
    )
    .await;
    let stopped = receive_command_ack(&mut socket).await;
    assert_eq!(
        stopped["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    socket.close().await;
    server.stop().await;
}

#[tokio::test]
async fn rejected_and_previous_generation_resume_retries_keep_lifecycle_results() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("create durable resume retry root: {error}"));
    let database_url = database_url(directory.path());
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open durable resume retry store: {error}"));
    bootstrap(&store).await;
    let staging_store = store.clone();
    let catalog = agent_catalog(directory.path());
    let first = start(store, catalog.clone()).await;
    let mut socket = connect(&first.base_url).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let rejected_session = create_stopped_agent(
        &mut socket,
        directory.path(),
        "create-rejected-resume-retry",
    )
    .await;
    let previous_session = create_stopped_agent(
        &mut socket,
        directory.path(),
        "create-previous-resume-retry",
    )
    .await;
    socket.close().await;
    first.stop().await;

    let rejected_payload = json!({"agent_id": rejected_session});
    let AgentStartPlan::Start(rejected_effect) = staging_store
        .prepare_agent_resume(
            &local_principal(),
            "rejected-resume-retry",
            &rejected_payload,
        )
        .await
        .unwrap_or_else(|error| panic!("stage rejected resume: {error}"))
    else {
        panic!("stopped session must stage a resume effect");
    };
    staging_store
        .fail_agent_start_before_effect(
            &local_principal(),
            "rejected-resume-retry",
            &rejected_payload,
            &rejected_effect.operation_id,
            "provider_login_required",
            "Provider login is required.",
            "agent.resume",
        )
        .await
        .unwrap_or_else(|error| panic!("reject staged resume: {error}"));
    drop(staging_store);

    let previous_owner = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open previous resume owner: {error}"));
    let previous_payload = json!({"agent_id": previous_session});
    assert!(matches!(
        previous_owner
            .prepare_agent_resume(
                &local_principal(),
                "previous-resume-retry",
                &previous_payload,
            )
            .await
            .unwrap_or_else(|error| panic!("stage previous-generation resume: {error}")),
        AgentStartPlan::Start(_)
    ));
    drop(previous_owner);

    let reopened = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("reopen durable resume retry store: {error}"));
    let second = start(reopened, catalog).await;
    let mut retry_socket = connect(&second.base_url).await;
    subscribe(&mut retry_socket).await;
    let _snapshot = receive_json(&mut retry_socket).await;

    send_command(
        &mut retry_socket,
        "rejected-resume-retry",
        "agent.resume",
        &rejected_payload,
    )
    .await;
    assert_eq!(
        receive_nack(&mut retry_socket).await["error"]["code"],
        "provider_login_required"
    );
    send_command(
        &mut retry_socket,
        "previous-resume-retry",
        "agent.resume",
        &previous_payload,
    )
    .await;
    assert_eq!(
        receive_nack(&mut retry_socket).await["error"]["code"],
        "runtime_start_abandoned_before_effect"
    );
    second.stop().await;
}

fn database_url(root: &Path) -> String {
    format!("sqlite://{}", root.join("runtime.sqlite3").display())
}

async fn create_stopped_agent<S>(
    socket: &mut AuthenticatedTestSocket<S>,
    workspace: &Path,
    request_id: &str,
) -> String
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_create(
        socket,
        request_id,
        &json!({
            "provider_id": "codex",
            "catalog_revision": "catalog-boundary-1",
            "display_name": request_id,
            "workspace": workspace,
            "model": "gpt-5.6-terra",
            "permission_mode": "meeting_read_only",
            "start_now": false,
        }),
    )
    .await;
    receive_command_ack(socket).await["result"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"))
        .to_owned()
}

async fn receive_nack<S>(socket: &mut AuthenticatedTestSocket<S>) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    for _ in 0..32 {
        let frame = receive_json(socket).await;
        if frame["op"] == "nack" {
            assert_ne!(frame["error"]["code"], "command_conflict");
            return frame;
        }
    }
    panic!("command NACK was not delivered");
}
