#![cfg(unix)]
use agentsassemble_persistence::AttendeeCleanupReport;
use agentsassemble_provider::{
    ProviderAdapter, ProviderAttachmentReadIngress, ProviderCatalogService, ProviderRoomToolIngress,
};
use agentsassemble_server::{
    AttendeeExecution, AttendeeRuntime, AttendeeSocket, AttendeeSocketFrame as Frame,
    AttendeeSocketRequest as Request, RoomAttendeeClient,
};
use serde_json::json;
use std::time::Duration;
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/local_socket.rs"]
mod local_socket;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_portal_fixture.rs"]
mod room_portal_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[path = "attendee_client_execution/tools.rs"]
mod tool_relay;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn external_execution_reconnects_without_reentry_and_recovers_committed_report() -> TestResult
{
    let directory = tempfile::tempdir()?;
    let [log, endpoint, token, seen, first, second] =
        ["log", "endpoint", "token", "seen", "first", "second"]
            .map(|name| directory.path().join(name));
    let script =
        room_portal_fixture::script(&log, &endpoint, &token, &seen, &first, &second, "completed");
    let mut fixture_catalog =
        provider_fixture::agent_catalog(directory.path(), Some(script.as_bytes()));
    // This controlled transport exercises the API/local persona contract. Real harness
    // registrations retain their separate selection-time persona rejection.
    fixture_catalog.providers[0].catalog_group = "api".to_owned();
    let catalog = ProviderCatalogService::fixed(fixture_catalog);
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let url = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let mut client = RoomAttendeeClient::new(&url, "codex", "External Execution")?;
    let mut runtime = prepare_runtime(&mut client, &catalog, directory.path())
        .await
        .map_err(|error| format!("initial local runtime: {error}"))?;
    let mut socket = ready_socket(&client, &mut runtime).await?;
    let (mut human, attachment_id) = send_input(&server, &store).await?;
    let Frame::Turn { assignment } =
        tokio::time::timeout(Duration::from_secs(10), socket.receive()).await??
    else {
        return Err("turn missing".into());
    };
    let (tools, mut tools_rx) = ProviderRoomToolIngress::channel(4);
    let (attachments, mut attachments_rx) = ProviderAttachmentReadIngress::channel(4);
    let mut execution = runtime
        .execute(*assignment, tools.clone(), attachments.clone(), None)
        .await
        .map_err(|error| format!("first local execution: {}", error.code))?;
    room_portal_fixture::wait_for_turn(&seen, "1").await;
    let mut replacement = ready_socket(&client, &mut runtime).await?;
    let Frame::Turn { mut assignment } =
        tokio::time::timeout(Duration::from_secs(10), replacement.receive()).await??
    else {
        return Err("resumed turn missing".into());
    };
    assert!(assignment.resume && execution.matches_delivery(&assignment));
    assert!(
        runtime
            .execute((*assignment).clone(), tools, attachments, None)
            .await
            .is_err()
    );
    assignment.input.provider_input.push('x');
    assert!(!execution.matches_delivery(&assignment));
    let endpoint = room_portal_fixture::wait_for_value(&endpoint, "endpoint").await;
    let token = room_portal_fixture::wait_for_value(&token, "token").await;
    tool_relay::verify(
        &client,
        replacement.connection_id(),
        &endpoint,
        &token,
        &mut tools_rx,
        (&attachment_id, &mut attachments_rx),
    )
    .await?;
    let view =
        room_portal_fixture::publish(&endpoint, &token, "Exactly one external execution").await;
    assert!(view.contains("Reply through the external runtime"));
    std::fs::write(first, b"go")?;
    tokio::time::timeout(Duration::from_secs(10), execution.complete()).await??;
    assert!(!execution.is_running());
    recover_report(&client, &mut runtime, &execution, &mut replacement, &server).await?;
    verify_committed_execution(&store, &log).await?;
    client.leave(Uuid::new_v4()).await?;
    runtime.stop().await?;
    let stopped = client.cleanup().await?.ok_or("cleanup missing")?;
    runtime.verify_cleanup(&stopped)?;
    client
        .report_cleanup(&AttendeeCleanupReport {
            request_id: Uuid::new_v4(),
            stopped: stopped.clone(),
        })
        .await?;
    runtime.acknowledge_cleanup(&stopped).await?;
    human.close().await;
    server.stop().await;
    Ok(())
}

async fn verify_committed_execution(
    store: &agentsassemble_persistence::SqliteStore,
    log: &std::path::Path,
) -> TestResult {
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("Exactly one external execution"))
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event
                .extra
                .get("message_source")
                .and_then(serde_json::Value::as_str)
                == Some("room_tool_result"))
            .count(),
        1
    );
    let provider_input = std::fs::read_to_string(log)?;
    assert_eq!(provider_input.matches("turn/start").count(), 1);
    assert!(provider_input.contains("Own-computer persona marker"));
    Ok(())
}

async fn ready_socket(
    client: &RoomAttendeeClient,
    runtime: &mut AttendeeRuntime,
) -> Result<AttendeeSocket, Box<dyn std::error::Error>> {
    let mut socket = client.connect().await?;
    socket
        .send(&Request::Ready {
            request_id: Uuid::new_v4(),
            report: Box::new(runtime.start().await?),
        })
        .await?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), socket.receive()).await??,
        Frame::Ack { .. }
    ));
    Ok(socket)
}

async fn recover_report(
    client: &RoomAttendeeClient,
    runtime: &mut AttendeeRuntime,
    execution: &AttendeeExecution,
    socket: &mut AttendeeSocket,
    server: &human_invite::RunningServer,
) -> TestResult {
    socket
        .send(&execution.started_request().ok_or("start report missing")?)
        .await?;
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(10), socket.receive()).await??,
        Frame::Ack { .. }
    ));
    let request = execution.report_request().ok_or("result report missing")?;
    let mut events = server.rooms().subscribe("general").await;
    socket.send(&request).await?;
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if events.recv().await?.content.as_deref() == Some("Exactly one external execution") {
                break;
            }
        }
        Ok::<_, tokio::sync::broadcast::error::RecvError>(())
    })
    .await??;
    // The result committed, but the client closes before consuming its acknowledgment.
    socket.close().await?;
    let mut replacement = client.connect().await?;
    replacement.send(&request).await?;
    assert!(
        matches!(tokio::time::timeout(Duration::from_secs(10), replacement.receive()).await??, Frame::Ack { request_id, deduplicated:Some(true), .. } if request_id == request.request_id())
    );
    execution.acknowledge(runtime, request.request_id()).await?;
    replacement.close().await?;
    Ok(())
}

async fn prepare_runtime(
    client: &mut RoomAttendeeClient,
    catalog: &ProviderCatalogService,
    workspace: &std::path::Path,
) -> Result<AttendeeRuntime, Box<dyn std::error::Error>> {
    let joined = client.join().await?;
    let draft: agentsassemble_domain::AgentSessionDraft = catalog
        .validate_creation(
            &joined.room_id,
            &joined.participant_id,
            &Uuid::new_v4().to_string(),
            &json!({
                "provider":"codex", "catalog_revision":catalog.snapshot().catalog_revision,
                "display_name":"External Execution", "workspace":workspace, "persona_card_id":"local-guide"
            }),
        )
        .await?
        .into();
    let adapter = ProviderAdapter::with_guardian_executable(std::path::Path::new(env!(
        "CARGO_BIN_EXE_agentsassemble-server"
    )));
    assert!(AttendeeRuntime::new(&joined, draft.clone(), adapter.clone(), None).is_err());
    let persona = serde_json::from_value(json!({
        "id":"local-guide", "display_name":"Local Guide", "description":"",
        "system_prompt":"Own-computer persona marker", "personality":"", "scenario":"",
        "first_message":"", "example_messages":"", "post_history_instructions":"",
        "lorebook":[], "lore_settings":{"scan_depth":1,"recursive_scanning":false,"full_word_matching":false},
        "asset_kind":"card", "source_kind":"fixture", "asset_count":0, "ignored_features":{}, "tag_count":0
    }))?;
    let mut runtime = AttendeeRuntime::new(&joined, draft, adapter, Some(persona))?;
    runtime.start().await?;
    Ok(runtime)
}

async fn send_input(
    server: &human_invite::RunningServer,
    store: &agentsassemble_persistence::SqliteStore,
) -> Result<
    (
        room_socket_peer::RoomSocketPeer<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
        String,
    ),
    Box<dyn std::error::Error>,
> {
    let mut human = local_socket::connect(&server.base_url, server.state(), "general").await;
    human.subscribe(0).await;
    let snapshot = human.receive_json().await;
    human.send_json(&json!({"op":"command", "request_id":"native-tools-mode", "action":"room.settings.update", "payload":{
        "expected_revision":snapshot["room_settings"]["settings_revision"], "tool_mode":"tabletop"
    }})).await;
    loop {
        let frame = human
            .receive_json_with_timeout(Duration::from_secs(2))
            .await;
        if frame["op"] == "ack" {
            break;
        }
        assert_ne!(frame["op"], "nack");
    }
    let attachment = store
        .store_local_message_attachment(
            "general",
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            "external-proof.txt",
            "text/plain",
            b"external attachment proof".to_vec(),
        )
        .await?;
    human.send_json(&json!({"op":"command", "request_id":"external-execution-input", "action":"message.send", "payload":{"content":"Reply through the external runtime", "attachment_ids":[attachment.id]}})).await;
    Ok((human, attachment.id))
}
