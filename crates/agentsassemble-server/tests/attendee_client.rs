use agentsassemble_persistence::AttendeeCleanupReport;
use agentsassemble_server::RoomAttendeeClient;
use uuid::Uuid;

#[path = "support/attendee.rs"]
mod attendee;
#[path = "support/human_invite.rs"]
mod human_invite;
#[path = "support/lossy_http.rs"]
mod lossy_http;
#[path = "support/room_socket_peer.rs"]
mod room_socket_peer;

#[tokio::test]
async fn attendee_client_recovers_lost_admission_leave_and_cleanup_acknowledgments()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let relay = lossy_http::LossyHttpRelay::start(
        &server.base_url,
        &[
            "/api/room-attendee/join",
            "/api/room-attendee/leave",
            "/api/room-attendee/cleanup",
        ],
    )
    .await?;
    let url = format!("{}/join?token={}", relay.base_url, invite.invite_bearer);
    let mut client = RoomAttendeeClient::for_room(
        &url,
        "codex",
        "Local Attendee Client",
        "general".to_owned(),
        invite.room_uid,
    )?;
    let error = client
        .join()
        .await
        .err()
        .ok_or("lost response should fail")?;
    assert_eq!(error.code, "invalid_attendee_response");
    assert!(!error.to_string().contains(&invite.invite_bearer));
    let joined = client.join().await?;
    assert_eq!(joined.provider_kind, "codex_live_session");
    assert_eq!(client.join().await?.participant_id, joined.participant_id);
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .len(),
        1
    );
    let leave_id = Uuid::new_v4();
    assert!(client.leave(leave_id).await.is_err());
    client.leave(leave_id).await?;
    let stopped = client.cleanup().await?.ok_or("cleanup missing")?;
    assert!(stopped.runtime_handle_id.is_empty());
    let report = AttendeeCleanupReport {
        request_id: Uuid::new_v4(),
        stopped,
    };
    assert!(client.report_cleanup(&report).await.is_err());
    client.report_cleanup(&report).await?;
    assert!(client.cleanup().await?.is_none());
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "participant_left")
            .count(),
        1
    );
    assert!(
        RoomAttendeeClient::new(
            "https://example.test/join?token=not-an-attendee",
            "codex",
            "AI"
        )
        .is_err()
    );
    relay.stop().await?;
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn local_handoff_rejects_an_admission_for_another_room_incarnation()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store).await;
    let url = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let mut client = RoomAttendeeClient::for_room(
        &url,
        "codex",
        "Bound local draft",
        "general".to_owned(),
        Uuid::new_v4(),
    )?;
    assert_eq!(
        client
            .join()
            .await
            .err()
            .ok_or("wrong incarnation was accepted")?
            .code,
        "invalid_attendee_admission"
    );
    assert!(client.connect().await.is_err());
    server.stop().await;
    Ok(())
}

#[tokio::test]
async fn attendee_native_socket_replaces_custody_and_keeps_runtime_until_exact_cleanup()
-> Result<(), Box<dyn std::error::Error>> {
    use agentsassemble_server::{AttendeeSocketFrame as Frame, AttendeeSocketRequest as Request};
    use std::time::Duration;
    let (store, invite) = attendee::fixture().await?;
    let server = human_invite::start(store.clone()).await;
    let url = format!("{}/join?token={}", server.base_url, invite.invite_bearer);
    let mut client = RoomAttendeeClient::new(&url, "codex", "Socket Client")?;
    assert!(client.connect().await.is_err());
    client.join().await?;
    let mut old = client.connect().await?;
    let request_id = Uuid::new_v4();
    let request: Request = serde_json::from_value(serde_json::json!({
        "action":"ready", "request_id":request_id, "report":{
            "runtime_handle_id":"client-runtime", "runtime_owner_id":"client-owner",
            "runtime_lease_token":"client-lease", "provider_session_id":"client-session",
            "model":"contract-model", "reasoning_effort":"", "service_tier":"", "variant":"",
            "execution_harness":"builtin", "permission_mode":"meeting_read_only",
            "max_output_tokens":0, "retained_interrupt":true
        }
    }))?;
    old.send(&request).await?;
    assert!(matches!(old.receive().await?, Frame::Ack { request_id: id, .. } if id == request_id));
    let mut current = client.connect().await?;
    assert_ne!(old.connection_id(), current.connection_id());
    assert!(
        tokio::time::timeout(Duration::from_secs(2), old.receive())
            .await?
            .is_err()
    );
    current.ping().await?;
    current.send(&request).await?;
    assert!(
        matches!(current.receive().await?, Frame::Ack { request_id: id, .. } if id == request_id)
    );
    current.close().await?;
    client.leave(Uuid::new_v4()).await?;
    let stopped = client.cleanup().await?.ok_or("cleanup missing")?;
    assert_eq!(stopped.runtime_handle_id, "client-runtime");
    assert_eq!(stopped.runtime_lease_token, "client-lease");
    client
        .report_cleanup(&AttendeeCleanupReport {
            request_id: Uuid::new_v4(),
            stopped,
        })
        .await?;
    assert!(client.cleanup().await?.is_none());
    server.stop().await;
    Ok(())
}
