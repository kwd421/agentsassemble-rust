use super::*;

#[tokio::test]
async fn idle_worker_exit_publishes_recovery_without_a_room_command() {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("failure root: {error}"));
    let store = SqliteStore::open(&format!(
        "sqlite://{}",
        directory.path().join("runtime.sqlite3").display()
    ))
    .await
    .unwrap_or_else(|error| panic!("failure store: {error}"));
    bootstrap(&store).await;
    let started = directory.path().join("native-starts");
    let fixture = start_marker_fixture(&started);
    let server = start(
        store,
        agent_catalog(directory.path(), Some(fixture.as_bytes())),
    )
    .await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    send_create(
        &mut socket,
        "idle-crash-create",
        &json!({
            "provider_id":"codex", "catalog_revision":"catalog-boundary-1",
            "display_name":"Idle peer", "workspace":directory.path(),
            "model":"gpt-5.6-terra", "permission_mode":"meeting_read_only", "start_now":true,
        }),
    )
    .await;
    let created = receive_until_ack(&mut socket, 5).await;
    let session_id = created["result"]["start"]["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session id"));
    let before = server
        .state
        .store
        .load_runtime_reconciliation_candidate("general", session_id)
        .await
        .unwrap_or_else(|error| panic!("idle candidate: {error}"))
        .unwrap_or_else(|| panic!("idle candidate missing"));
    assert_eq!(
        before.session.public.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Idle
    );
    let killed_at = kill_owned_worker(&started).await;

    // No post-kill message, lifecycle command, or health request may cause this event.
    let state = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = receive_json(&mut socket).await;
            if let Some(events) = frame["events"].as_array() {
                for event in events {
                    if event["type"] == "agent_session_state"
                        && event["agent_session"]["last_error_code"] == "managed_bridge_exited"
                    {
                        return event["agent_session"].clone();
                    }
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("idle worker exit was not published"));
    eprintln!(
        "idle_bridge_failure_publication_ms={:.3}",
        killed_at.elapsed().as_secs_f64() * 1000.0
    );
    assert_eq!(state["recovery_required"], true);
    assert_eq!(state["enabled"], false);
    assert_eq!(state["provider_session_active"], false);
    let after = server
        .state
        .store
        .load_runtime_reconciliation_candidate("general", session_id)
        .await
        .unwrap_or_else(|error| panic!("failed candidate: {error}"))
        .unwrap_or_else(|| panic!("failure lost runtime custody"));
    assert_eq!(
        before.session.runtime_handle_id,
        after.session.runtime_handle_id
    );
    assert_eq!(
        before.session.runtime_owner_id,
        after.session.runtime_owner_id
    );
    assert_eq!(
        before.session.runtime_lease_token,
        after.session.runtime_lease_token
    );
    assert_eq!(
        std::fs::read_to_string(&started)
            .unwrap_or_else(|error| panic!("native start count: {error}"))
            .lines()
            .count(),
        1
    );
    confirm_stop(&mut socket, session_id).await;
    server.stop().await;
}

async fn parent_pid(pid: u32) -> u32 {
    process_field(pid, "ppid=")
        .await
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("owned process parent: {error}"))
}

async fn process_field(pid: u32, field: &str) -> String {
    let output = tokio::process::Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", field])
        .output()
        .await
        .unwrap_or_else(|error| panic!("owned process observation: {error}"));
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .unwrap_or_else(|error| panic!("process observation text: {error}"))
}

fn start_marker_fixture(started: &Path) -> String {
    std::str::from_utf8(provider_fixture::CODEX_FIXTURE)
        .unwrap_or_else(|error| panic!("fixture text: {error}"))
        .replacen(
            "#!/bin/sh\n",
            &format!(
                "#!/bin/sh\nprintf '%s\\n' \"$$\" >> {}\n",
                shell_quote(started)
            ),
            1,
        )
}

async fn kill_owned_worker(started: &Path) -> std::time::Instant {
    let native_pid: u32 = std::fs::read_to_string(started)
        .unwrap_or_else(|error| panic!("native start: {error}"))
        .trim()
        .parse()
        .unwrap_or_else(|error| panic!("native pid: {error}"));
    let guardian = parent_pid(native_pid).await;
    let worker = parent_pid(guardian).await;
    assert_eq!(parent_pid(worker).await, std::process::id());
    let command = process_field(worker, "command=").await;
    assert!(command.contains("--agentsassemble-managed-provider"));
    let started_at = std::time::Instant::now();
    let killed = tokio::process::Command::new("kill")
        .args(["-KILL", &worker.to_string()])
        .status()
        .await
        .unwrap_or_else(|error| panic!("kill owned worker: {error}"));
    assert!(killed.success());
    started_at
}

async fn confirm_stop<S>(socket: &mut RoomSocketPeer<S>, session_id: &str)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_command(
        socket,
        "idle-crash-stop",
        "agent.stop",
        &json!({"agent_id":session_id}),
    )
    .await;
    let stopped = receive_stop_receipt(
        socket,
        "idle-crash-stop",
        session_id,
        "managed_bridge_protocol_failed",
    )
    .await;
    assert_eq!(
        stopped["result"]["agent_session"]["runtime_status"],
        "stopped"
    );
    assert_eq!(
        stopped["result"]["agent_session"]["recovery_required"],
        false
    );
}
