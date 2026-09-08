use agentsassemble_persistence::AgentStartPlan;
use agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal;

use super::*;

#[tokio::test]
async fn current_generation_launch_retry_reaches_its_lifecycle_owner() {
    for action in ["agent.resume", "agent.readd"] {
        current_generation_launch_retry(action).await;
    }
}

async fn current_generation_launch_retry(action: &'static str) {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create resume retry root: {error}"));
    let database_url = database_url(directory.path());
    let store = SqliteStore::open(&database_url)
        .await
        .unwrap_or_else(|error| panic!("open resume retry store: {error}"));
    bootstrap(&store).await;
    let staging_store = store.clone();
    let server = start(store, agent_catalog(directory.path(), None)).await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let session_id =
        create_stopped_agent(&mut socket, directory.path(), "create-current-resume-retry").await;
    let payload = launch_payload(action, &session_id);

    assert!(matches!(
        staging_store
            .prepare_agent_launch(
                TrustedPrincipal(&local_principal()),
                "current-resume-retry",
                &payload,
                action
            )
            .await
            .unwrap_or_else(|error| panic!("stage current resume: {error}")),
        AgentStartPlan::Start(_)
    ));
    send_command(&mut socket, "current-resume-retry", action, &payload).await;
    let resumed = receive_command_ack(&mut socket).await;
    assert_eq!(resumed["result"]["agent_session"]["runtime_status"], "idle");
    assert_eq!(
        resumed["result"]["event_seq"],
        resumed["result"]["event"]["seq"]
    );
    send_command(&mut socket, "current-resume-retry", action, &payload).await;
    assert_eq!(receive_command_ack(&mut socket).await["deduplicated"], true);

    send_command(
        &mut socket,
        "stop-current-resume-retry",
        "agent.stop",
        &json!({"agent_id": session_id}),
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
async fn rejected_and_previous_generation_launch_retries_keep_lifecycle_results() {
    for action in ["agent.resume", "agent.readd"] {
        rejected_and_previous_generation_launch_retry(action)
            .await
            .unwrap_or_else(|error| panic!("launch retry: {error}"));
    }
}

async fn rejected_and_previous_generation_launch_retry(
    action: &'static str,
) -> Result<(), Box<dyn std::error::Error>> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let database_url = database_url(directory.path());
    let store = SqliteStore::open(&database_url).await?;
    bootstrap(&store).await;
    let staging_store = store.clone();
    let catalog = agent_catalog(directory.path(), None);
    let first = start(store, catalog.clone()).await;
    let mut socket = connect(&first.base_url, &first.state).await;
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

    let rejected_payload = launch_payload(action, &rejected_session);
    let AgentStartPlan::Start(rejected_effect) = staging_store
        .prepare_agent_launch(
            TrustedPrincipal(&local_principal()),
            "rejected-resume-retry",
            &rejected_payload,
            action,
        )
        .await?
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
            action,
        )
        .await?;
    drop(staging_store);

    let previous_owner = SqliteStore::open(&database_url).await?;
    let previous_payload = launch_payload(action, &previous_session);
    assert!(matches!(
        previous_owner
            .prepare_agent_launch(
                TrustedPrincipal(&local_principal()),
                "previous-resume-retry",
                &previous_payload,
                action,
            )
            .await?,
        AgentStartPlan::Start(_)
    ));
    drop(previous_owner);

    let reopened = SqliteStore::open(&database_url).await?;
    let second = start(reopened, catalog).await;
    let mut retry_socket = connect(&second.base_url, &second.state).await;
    subscribe(&mut retry_socket).await;
    let _snapshot = receive_json(&mut retry_socket).await;

    send_command(
        &mut retry_socket,
        "rejected-resume-retry",
        action,
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
        action,
        &previous_payload,
    )
    .await;
    assert_eq!(
        receive_nack(&mut retry_socket).await["error"]["code"],
        "runtime_start_abandoned_before_effect"
    );
    second.stop().await;
    Ok(())
}

fn database_url(root: &Path) -> String {
    format!("sqlite://{}", root.join("runtime.sqlite3").display())
}

pub(super) async fn create_stopped_agent<S>(
    socket: &mut RoomSocketPeer<S>,
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

async fn receive_nack<S>(socket: &mut RoomSocketPeer<S>) -> Value
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

fn launch_payload(action: &str, session_id: &str) -> Value {
    let mut payload = json!({"agent_id": session_id});
    if action == "agent.readd" {
        payload["start"] = json!(true);
    }
    payload
}

#[tokio::test]
async fn listing_readd_replays_across_socket_reconnect_and_server_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let database_url = database_url(directory.path());
    let store = SqliteStore::open(&database_url).await?;
    bootstrap(&store).await;
    let catalog = agent_catalog(directory.path(), None);
    let first = start(store, catalog.clone()).await;
    let mut socket = connect(&first.base_url, &first.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let session_id = create_stopped_agent(&mut socket, directory.path(), "create-for-readd").await;
    let payload = json!({"agent_id": session_id, "start": false});
    send_command(&mut socket, "listing-readd", "agent.readd", &payload).await;
    let added = receive_command_ack(&mut socket).await;
    assert_eq!(added["result"]["status"], "readded");
    assert_eq!(
        added["result"]["event_seq"],
        added["result"]["event"]["seq"]
    );
    assert_eq!(added["result"]["participant"]["status"], "detached");
    assert_eq!(added["result"]["agent_session"]["enabled"], false);
    assert_eq!(added["result"]["events"].as_array().map(Vec::len), Some(1));
    socket.close().await;
    let mut reconnected = connect(&first.base_url, &first.state).await;
    subscribe(&mut reconnected).await;
    let snapshot = receive_json(&mut reconnected).await;
    assert_eq!(
        snapshot["agent_sessions"][0],
        added["result"]["agent_session"]
    );
    send_command(&mut reconnected, "listing-readd", "agent.readd", &payload).await;
    assert_eq!(
        receive_command_ack(&mut reconnected).await["deduplicated"],
        true
    );
    send_command(
        &mut reconnected,
        "listing-readd",
        "agent.readd",
        &json!({"agent_id": session_id, "start": true}),
    )
    .await;
    assert_eq!(
        receive_json(&mut reconnected).await["error"]["code"],
        "command_conflict"
    );
    reconnected.close().await;
    first.stop().await;
    let reopened = SqliteStore::open(&database_url).await?;
    let second = start(reopened, catalog).await;
    let mut socket = connect(&second.base_url, &second.state).await;
    subscribe(&mut socket).await;
    let snapshot = receive_json(&mut socket).await;
    assert_eq!(
        snapshot["agent_sessions"][0],
        added["result"]["agent_session"]
    );
    send_command(&mut socket, "listing-readd", "agent.readd", &payload).await;
    let replay = receive_command_ack(&mut socket).await;
    assert_eq!(replay["deduplicated"], true);
    assert_eq!(replay["result"], added["result"]);
    socket.close().await;
    second.stop().await;
    Ok(())
}
