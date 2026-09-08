#![cfg(unix)]
use agentsassemble_persistence::AttendeeCleanupReport;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use agentsassemble_server::{
    AttendeeRuntime, AttendeeSocketFrame, AttendeeSocketRequest, RoomAttendeeClient,
};
use serde_json::json;
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/provider_fixture.rs"]
mod provider_fixture;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[tokio::test]
async fn external_client_owns_fixture_process_and_only_reports_exact_confirmed_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let catalog = ProviderCatalogService::fixed(provider_fixture::agent_catalog(directory.path()));
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let url = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let mut client = RoomAttendeeClient::new(&url, "codex", "Owned Local Runtime")?;
    let joined = client.join().await?;
    let draft = catalog
        .validate_creation(
            &joined.room_id,
            &joined.participant_id,
            &Uuid::new_v4().to_string(),
            &json!({
                "provider":"codex", "catalog_revision":catalog.snapshot().catalog_revision,
                "display_name":"Owned Local Runtime", "workspace":directory.path()
            }),
        )
        .await?
        .into();
    let adapter = ProviderAdapter::with_guardian_executable(std::path::Path::new(env!(
        "CARGO_BIN_EXE_agentsassemble-server"
    )));
    let mut runtime = AttendeeRuntime::new(&joined, draft, adapter)?;
    let ready = runtime.start().await?;
    assert!(ready.retained_interrupt);
    assert!(!ready.runtime_handle_id.is_empty());
    assert_eq!(
        runtime.start().await?.runtime_handle_id,
        ready.runtime_handle_id
    );
    let mut socket = client.connect().await?;
    socket
        .send(&AttendeeSocketRequest::Ready {
            request_id: Uuid::new_v4(),
            report: Box::new(ready.clone()),
        })
        .await?;
    assert!(matches!(
        socket.receive().await?,
        AttendeeSocketFrame::Ack { .. }
    ));
    client.leave(Uuid::new_v4()).await?;
    let stopped = client.cleanup().await?.ok_or("cleanup missing")?;
    assert_eq!(stopped.runtime_handle_id, ready.runtime_handle_id);
    assert!(runtime.verify_cleanup(&stopped).is_err());
    runtime.stop().await?;
    runtime.stop().await?;
    runtime.verify_cleanup(&stopped)?;
    let mut wrong = stopped.clone();
    wrong.runtime_lease_token.push('x');
    assert!(runtime.verify_cleanup(&wrong).is_err());
    client
        .report_cleanup(&AttendeeCleanupReport {
            request_id: Uuid::new_v4(),
            stopped: stopped.clone(),
        })
        .await?;
    runtime.acknowledge_cleanup(&stopped).await?;
    assert!(client.cleanup().await?.is_none());
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert!(!snapshot.agent_sessions[0].provider_session_active);
    assert!(runtime.start().await.is_err());
    server.stop().await;
    Ok(())
}
