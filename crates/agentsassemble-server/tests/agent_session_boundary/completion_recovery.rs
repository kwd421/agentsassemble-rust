use super::*;
use agentsassemble_provider::ProviderExactTurnAuthority;
use anyhow::Context;
use rmcp::{
    ServiceExt,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};

#[tokio::test]
async fn completion_recovery_retains_result_until_publication_target_is_unmuted()
-> anyhow::Result<()> {
    let _serial = AGENT_BOUNDARY_LOCK.lock().await;
    let directory = tempfile::tempdir()?;
    let root = directory.path();
    let transcript = root.join("requests.jsonl");
    let endpoint_file = root.join("portal-endpoint");
    let token_file = root.join("portal-token");
    let seen = root.join("turn-seen");
    let release = root.join("release");
    let script = room_portal_fixture::script(
        &transcript,
        &endpoint_file,
        &token_file,
        &seen,
        &release,
        &root.join("unused-release"),
        "completed",
    );
    let store = SqliteStore::open_path(&root.join("runtime.sqlite3")).await?;
    bootstrap(&store).await;
    let server = start(store, agent_catalog(root, Some(script.as_bytes()))).await;
    let mut socket = connect(&server.base_url, &server.state).await;
    subscribe(&mut socket).await;
    let _snapshot = receive_json(&mut socket).await;
    let target_id = create_agent(&mut socket, root, "Target", true).await?;
    send_command(
        &mut socket,
        "pause-target",
        "agent.pause",
        &json!({"agent_id":target_id}),
    )
    .await;
    let _paused = command_ack(&mut socket).await?;
    let session_id = create_agent(&mut socket, root, "Terra", true).await?;
    send_command(
        &mut socket,
        "completion-source",
        "message.send",
        &json!({"content":"@Terra observe the completion recovery source"}),
    )
    .await;
    let _ack = command_ack(&mut socket).await?;
    room_portal_fixture::wait_for_turn(&seen, "1").await;
    let endpoint = room_portal_fixture::wait_for_value(&endpoint_file, "endpoint").await;
    let token = room_portal_fixture::wait_for_value(&token_file, "token").await;
    stage_publication(&endpoint, &token, &target_id).await?;
    // A real room command invalidates the staged target before provider completion.
    send_command(
        &mut socket,
        "mute-target",
        "participant.mute",
        &json!({"participant_id":target_id, "muted":true}),
    )
    .await;
    let _muted = command_ack(&mut socket).await?;
    std::fs::write(&release, b"release")?;
    let recovery = wait_for_state(&mut socket, &session_id, true).await;
    assert_eq!(recovery["runtime_status"], "busy");
    assert_eq!(
        recovery["last_error_code"],
        "provider_turn_recovery_required"
    );
    assert!(
        recovery["last_error"]
            .as_str()
            .context("recovery diagnostic")?
            .contains("room_portal_publication_invalid")
    );
    assert_eq!(recovery["last_provider_sync_seq"], 0);
    verify_recovery(&server, &mut socket, &session_id, &target_id, &transcript).await?;
    socket.close().await;
    server.stop().await;
    Ok(())
}

async fn create_agent<S>(
    socket: &mut RoomSocketPeer<S>,
    root: &Path,
    name: &str,
    start_now: bool,
) -> anyhow::Result<String>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    send_create(socket, &format!("create-{name}"), &json!({
        "provider_id":"codex", "catalog_revision":"catalog-boundary-1", "display_name":name,
        "workspace":root, "model":"gpt-5.6-terra", "permission_mode":"meeting_read_only", "start_now":start_now,
    })).await;
    let created = command_ack(socket).await?;
    let result = if start_now {
        &created["result"]["start"]
    } else {
        &created["result"]
    };
    Ok(result["agent_session"]["session_id"]
        .as_str()
        .context("started session id")?
        .to_owned())
}

async fn stage_publication(endpoint: &str, token: &str, target_id: &str) -> anyhow::Result<()> {
    let client = ()
        .serve(StreamableHttpClientTransport::from_config(
            StreamableHttpClientTransportConfig::with_uri(endpoint).auth_header(token),
        ))
        .await?;
    assert_ne!(
        room_portal_fixture::call_tool(&client, "read_discussion", json!({}))
            .await
            .is_error,
        Some(true)
    );
    assert_ne!(
        room_portal_fixture::call_tool(
            &client,
            "publish_message",
            json!({"content":"retained final answer", "next_agent_id":target_id})
        )
        .await
        .is_error,
        Some(true)
    );
    client.cancel().await?;
    Ok(())
}

async fn wait_for_state<S>(
    socket: &mut RoomSocketPeer<S>,
    session_id: &str,
    recovery: bool,
) -> Value
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let frame = receive_json(socket).await;
            for event in frame["events"].as_array().into_iter().flatten() {
                let session = &event["agent_session"];
                if event["type"] == "agent_session_state"
                    && session["session_id"] == session_id
                    && session["recovery_required"] == recovery
                    && (recovery || session["runtime_status"] == "idle")
                {
                    return session.clone();
                }
            }
        }
    })
    .await
    .unwrap_or_else(|_| panic!("expected recovery={recovery} state was not published"))
}

async fn verify_recovery<S>(
    server: &RunningServer,
    socket: &mut RoomSocketPeer<S>,
    session_id: &str,
    target_id: &str,
    transcript: &Path,
) -> anyhow::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let page = server
        .state
        .store
        .load_provider_turn_reconciliation_page(None)
        .await?;
    let candidate = page
        .candidates
        .iter()
        .find(|candidate| candidate.execution.session_id == session_id)
        .context("quarantined execution")?;
    let execution = &candidate.execution;
    let authority = ProviderExactTurnAuthority {
        room_id: execution.room_id.clone(),
        session_id: execution.session_id.clone(),
        execution_id: execution.execution_id.clone(),
        turn_id: execution.turn_id.clone(),
        turn_generation: execution.turn_generation,
        runtime_handle_id: execution.runtime_handle_id.clone(),
        runtime_owner_id: execution.runtime_owner_id.clone(),
        runtime_lease_token: execution.runtime_lease_token.clone(),
    };
    assert!(matches!(
        server
            .provider_adapter
            .retained_turn_result(&authority)
            .await,
        Some(Ok(_))
    ));
    send_command(
        socket,
        "unmute-target",
        "participant.mute",
        &json!({"participant_id":target_id, "muted":false}),
    )
    .await;
    let _unmuted = command_ack(socket).await?;
    let idle = wait_for_state(socket, session_id, false).await;
    assert_eq!(idle["runtime_status"], "idle");
    assert_eq!(idle["turn_count"], 1);
    assert_eq!(idle["last_error_code"], "");
    let snapshot = server.state.store.snapshot("general", 0, 200).await?;
    let source = snapshot
        .events
        .iter()
        .find(|event| {
            event.content.as_deref() == Some("@Terra observe the completion recovery source")
        })
        .context("source message")?;
    assert_eq!(idle["last_provider_sync_seq"], source.seq);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("retained final answer"))
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "agent_session_state"
                && event
                    .extra
                    .get("agent_session")
                    .and_then(|session| session.get("recovery_required"))
                    == Some(&json!(true)))
            .count(),
        1
    );
    assert!(
        server
            .provider_adapter
            .retained_turn_result(&authority)
            .await
            .is_none()
    );
    assert!(!server.provider_adapter.owns_exact_turn(&authority).await);
    assert_single_provider_turn(transcript)
}

fn assert_single_provider_turn(transcript: &Path) -> anyhow::Result<()> {
    let requests = std::fs::read_to_string(transcript)?;
    let methods = requests
        .lines()
        .map(serde_json::from_str::<Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(
        methods
            .iter()
            .filter(|request| request["method"] == "turn/start")
            .count(),
        1
    );
    Ok(())
}

async fn command_ack<S>(socket: &mut RoomSocketPeer<S>) -> anyhow::Result<Value>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let frame = receive_json(socket).await;
            if frame["op"] == "ack" {
                return Ok(frame);
            }
            if frame["op"] == "nack" {
                anyhow::bail!(
                    "fixture command {} rejected: {}",
                    frame["request_id"],
                    frame["error"]
                );
            }
        }
    })
    .await
    .context("fixture command acknowledgement timed out")?
}
