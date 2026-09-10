#![cfg(unix)]
use agentsassemble_persistence::{AttendeeCleanupReport, AttendeeInterruptedRuntime};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAttachmentReadIngress, ProviderCatalogService, ProviderRoomToolIngress,
};
use agentsassemble_server::{
    AttendeeRuntime, AttendeeSocket, AttendeeSocketFrame as Frame,
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
#[path = "support/native_interrupt_fixture.rs"]
mod native_interrupt_fixture;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn external_native_interrupt_preserves_retained_and_uncertain_custody() -> TestResult {
    for (entered, unconfirmed) in [(false, false), (true, false), (true, true)] {
        Box::pin(run_case(entered, unconfirmed)).await?;
    }
    Ok(())
}

async fn run_case(entered: bool, unconfirmed: bool) -> TestResult {
    let directory = tempfile::tempdir()?;
    let log = directory.path().join("requests");
    let seen = directory.path().join("seen");
    let script = native_interrupt_fixture::script(&log, &seen, unconfirmed);
    let catalog = ProviderCatalogService::fixed(provider_fixture::agent_catalog(
        directory.path(),
        Some(script.as_bytes()),
    ));
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let mut client = RoomAttendeeClient::new(
        &format!("{}/join?token={}", server.base_url, invite.invite_bearer),
        "codex",
        "Interrupt Client",
    )?;
    let mut runtime = start_runtime(&mut client, &catalog, directory.path()).await?;
    let mut socket = connect_ready(&client, &mut runtime).await?;
    let mut manager = local_socket::connect(&server.base_url, server.state(), "general").await;
    manager.subscribe(0).await;
    let _snapshot = manager.receive_json().await;
    manager.send_json(&json!({"op":"command", "request_id":"external-interrupt-input", "action":"message.send", "payload":{"content":"Stay active until interrupted"}})).await;
    let Frame::Turn { assignment } = receive(&mut socket).await? else {
        return Err("assignment missing".into());
    };
    let session_id = assignment.authority.session_id.clone();
    let (tools, _tools_rx) = ProviderRoomToolIngress::channel(4);
    let (attachments, _attachments_rx) = ProviderAttachmentReadIngress::channel(4);
    let mut execution = if entered {
        runtime
            .execute(*assignment, tools, attachments, None)
            .await
            .map(Some)?
    } else {
        None
    };
    if entered {
        wait_seen(&seen).await?;
    }
    manager.send_json(&json!({"op":"command", "request_id":"external-native-interrupt", "action":"agent.interrupt", "payload":{"agent_id":session_id}})).await;
    let Frame::Interrupt { interrupt } = receive(&mut socket).await? else {
        return Err("interrupt missing".into());
    };
    let mut wrong = (*interrupt).clone();
    wrong.authority.runtime_lease_token.push('x');
    assert!(runtime.interrupt(wrong, execution.as_ref()).is_err());
    let mut owned = runtime.interrupt(*interrupt, execution.as_ref())?;
    // Interrupt delivery precedes readiness on replacement; it must not enter another turn.
    let mut replacement = client.connect().await?;
    let Frame::Interrupt { interrupt } = receive(&mut replacement).await? else {
        return Err("recovered interrupt missing".into());
    };
    assert!(owned.matches_delivery(&interrupt));
    let completion = tokio::time::timeout(Duration::from_secs(20), owned.complete()).await?;
    if unconfirmed {
        let error = completion
            .err()
            .ok_or("uncertain provider interrupt unexpectedly succeeded")?;
        assert_eq!(error.code, "provider_turn_interrupt_unconfirmed");
        assert!(owned.report().is_none());
    } else {
        completion?;
        let report = owned.report().ok_or("quiescence missing")?;
        assert!(matches!(
            report.runtime,
            AttendeeInterruptedRuntime::Retained
        ));
        client
            .report_interrupt(replacement.connection_id(), report)
            .await?;
        client
            .report_interrupt(replacement.connection_id(), report)
            .await?;
        owned.acknowledge(&mut runtime, execution.as_mut()).await?;
        assert!(runtime.interrupt(*interrupt, None).is_err());
    }
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(snapshot.agent_sessions[0].provider_session_active);
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        usize::from(!unconfirmed)
    );
    let transcript = std::fs::read_to_string(log)?;
    for method in ["turn/start", "turn/interrupt"] {
        assert_eq!(transcript.matches(method).count(), usize::from(entered));
    }
    cleanup(&client, &mut runtime).await?;
    manager.close().await;
    server.stop().await;
    Ok(())
}

async fn connect_ready(
    client: &RoomAttendeeClient,
    runtime: &mut AttendeeRuntime,
) -> Result<AttendeeSocket, Box<dyn std::error::Error>> {
    let mut socket = client.connect().await?;
    socket
        .send(&Request::Ready {
            request_id: Uuid::new_v4(),
            report: Box::new(runtime.start(None).await?),
        })
        .await?;
    assert!(matches!(receive(&mut socket).await?, Frame::Ack { .. }));
    Ok(socket)
}

async fn start_runtime(
    client: &mut RoomAttendeeClient,
    catalog: &ProviderCatalogService,
    workspace: &std::path::Path,
) -> Result<AttendeeRuntime, Box<dyn std::error::Error>> {
    let joined = client.join().await?;
    let draft = catalog.validate_creation(&joined.room_id, &joined.participant_id, &Uuid::new_v4().to_string(), &json!({
        "provider":"codex", "catalog_revision":catalog.snapshot().catalog_revision, "display_name":"Interrupt Client", "workspace":workspace
    })).await?.into();
    let adapter = ProviderAdapter::with_guardian_executable(std::path::Path::new(env!(
        "CARGO_BIN_EXE_agentsassemble-server"
    )));
    let mut runtime = AttendeeRuntime::new(&joined, draft, adapter, None)?;
    runtime.start(None).await?;
    Ok(runtime)
}

async fn receive(socket: &mut AttendeeSocket) -> Result<Frame, Box<dyn std::error::Error>> {
    Ok(tokio::time::timeout(Duration::from_secs(10), socket.receive()).await??)
}

async fn wait_seen(path: &std::path::Path) -> TestResult {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if std::fs::read_to_string(path).is_ok_and(|value| value == "seen") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    Ok(())
}

async fn cleanup(client: &RoomAttendeeClient, runtime: &mut AttendeeRuntime) -> TestResult {
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
    runtime.acknowledge_cleanup(Some(&stopped)).await?;
    assert!(client.cleanup().await?.is_none());
    Ok(())
}
