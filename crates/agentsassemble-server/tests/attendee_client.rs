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
    let mut client = RoomAttendeeClient::new(&url, "codex", "Local Attendee Client")?;
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
