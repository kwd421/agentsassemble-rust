use crate::{
    AttendeeCleanupReport, RoomMutationAuthority, attendee_turn_report_tests::assigned_report,
    human_session_authority_tests::local_operator_principal,
};
use agentsassemble_domain::{AgentRuntimeStatus, ParticipantStatus};
use serde_json::json;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn expired_attendee_can_leave_once_without_fabricating_runtime_absence() -> TestResult {
    let (store, connection, turn, now) = assigned_report().await?;
    let fingerprint = connection.session().session_fingerprint();
    sqlx::query(
        "UPDATE room_attendee_invites SET session_expires_at=? WHERE session_fingerprint=?",
    )
    .bind((now - chrono::Duration::seconds(1)).timestamp_micros())
    .bind(fingerprint.as_slice())
    .execute(&store.pool)
    .await?;
    assert!(
        store
            .authorize_attendee_session(fingerprint, now)
            .await
            .is_err()
    );
    let authority = store.authorize_attendee_cleanup(fingerprint).await?;
    let request = Uuid::new_v4();
    let (one, two) = tokio::join!(
        store.leave_attendee(&authority, request),
        store.leave_attendee(&authority, request),
    );
    let one = one?;
    let two = two?;
    assert_ne!(one.outcome.deduplicated, two.outcome.deduplicated);
    assert_eq!(one.outcome.event.id, two.outcome.event.id);
    assert_eq!(one.outcome.event.event_type, "participant_left");
    assert_eq!(one.outcome.event.participant_type.as_deref(), Some("agent"));
    assert!(
        store
            .leave_attendee(&authority, Uuid::new_v4())
            .await
            .is_err()
    );
    let stopped = store
        .load_attendee_cleanup(&authority)
        .await?
        .ok_or("cleanup missing")?;
    assert_eq!(stopped.runtime_handle_id, turn.runtime_handle_id);
    assert!(
        store
            .finish_room_runtime_cleanup(&authority.key)
            .await?
            .is_none()
    );
    assert!(
        store
            .record_attendee_turn_report(&connection, &turn, now)
            .await
            .is_err()
    );
    store
        .record_attendee_cleanup(
            &authority,
            &AttendeeCleanupReport {
                request_id: Uuid::new_v4(),
                stopped,
            },
        )
        .await?;
    let replay = store.leave_attendee(&authority, request).await?;
    assert!(replay.outcome.deduplicated);
    assert!(store.load_attendee_cleanup(&authority).await?.is_none());
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "participant_left")
            .count(),
        1
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    assert!(
        snapshot
            .participants
            .iter()
            .any(
                |participant| participant.participant_id == authority.key.session_id
                    && participant.status == ParticipantStatus::Left
            )
    );
    let encoded = serde_json::to_string(&one.outcome.result)?;
    assert!(!encoded.contains(&turn.runtime_lease_token));
    Ok(())
}

#[tokio::test]
async fn revoked_attendee_cleanup_requires_its_exact_external_stop_and_retries_atomically()
-> TestResult {
    let (store, connection, turn, now) = assigned_report().await?;
    let fingerprint = connection.session().session_fingerprint();
    let authority = store.authorize_attendee_cleanup(fingerprint).await?;
    assert!(store.load_attendee_cleanup(&authority).await?.is_none());
    let operator = local_operator_principal();
    let removal = store
        .execute_participant_removal(
            RoomMutationAuthority::TrustedPrincipal(&operator),
            "external-cleanup-kick",
            "participant.kick",
            &json!({"participant_id":connection.session().principal().participant_id}),
        )
        .await?;
    let key = removal.cleanup.ok_or("cleanup pending missing")?;
    assert!(
        store
            .authorize_attendee_session(fingerprint, now)
            .await
            .is_err()
    );
    assert!(
        store
            .record_attendee_turn_report(&connection, &turn, now)
            .await
            .is_err()
    );
    assert!(store.load_room_runtime_cleanup_turn(&key).await?.is_none());
    assert!(
        store
            .load_room_runtime_cleanup_candidate(&key)
            .await?
            .is_none()
    );
    assert!(store.finish_room_runtime_cleanup(&key).await?.is_none());
    let authority = store.authorize_attendee_cleanup(fingerprint).await?;
    let delivery = store
        .load_attendee_cleanup(&authority)
        .await?
        .ok_or("stop request missing")?;
    assert_eq!(delivery.runtime_handle_id, turn.runtime_handle_id);
    let mut report = AttendeeCleanupReport {
        request_id: Uuid::new_v4(),
        stopped: delivery,
    };
    let mut stale = report.clone();
    stale.stopped.cleanup_id = Uuid::new_v4();
    assert!(
        store
            .record_attendee_cleanup(&authority, &stale)
            .await
            .is_err()
    );
    stale = report.clone();
    stale.stopped.runtime_lease_token = "different-runtime".to_owned();
    assert!(
        store
            .record_attendee_cleanup(&authority, &stale)
            .await
            .is_err()
    );
    assert!(store.finish_room_runtime_cleanup(&key).await?.is_none());
    let (one, two) = tokio::join!(
        store.record_attendee_cleanup(&authority, &report),
        store.record_attendee_cleanup(&authority, &report)
    );
    let one = one?;
    let two = two?;
    assert_ne!(one.outcome.deduplicated, two.outcome.deduplicated);
    assert_eq!(one.outcome.event.id, two.outcome.event.id);
    assert!(store.load_attendee_cleanup(&authority).await?.is_none());
    let snapshot = store.snapshot("general", 0, 200).await?;
    let session = snapshot.agent_sessions.first().ok_or("session missing")?;
    assert_eq!(session.runtime_status, AgentRuntimeStatus::Stopped);
    assert!(!session.provider_session_active);
    assert!(session.active_turn_id.is_empty());
    assert!(
        snapshot
            .participants
            .iter()
            .any(|participant| participant.participant_id == key.session_id
                && participant.status == ParticipantStatus::Kicked)
    );
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    let encoded = serde_json::to_string(&one.outcome.result)?;
    assert!(!encoded.contains(&report.stopped.runtime_lease_token));
    report.stopped.cleanup_id = Uuid::new_v4();
    assert!(matches!(
        store.record_attendee_cleanup(&authority, &report).await,
        Err(crate::PersistenceError::CommandConflict)
    ));
    assert!(store.authorize_attendee_cleanup(&[0xAD; 32]).await.is_err());
    Ok(())
}

#[tokio::test]
async fn idle_external_cleanup_preserves_removal_and_unstarted_cleanup_needs_no_runtime_claim()
-> TestResult {
    for ready in [false, true] {
        let (store, session, now) = crate::attendee_connection_tests::fixture().await?;
        let connection = store
            .claim_attendee_connection(&session, Uuid::new_v4(), now)
            .await?
            .authorization;
        if ready {
            store
                .record_attendee_ready(&connection, &crate::attendee_ready_tests::report(), now)
                .await?;
        }
        let operator = local_operator_principal();
        let removal = store
            .execute_participant_removal(
                RoomMutationAuthority::TrustedPrincipal(&operator),
                "idle-external-kick",
                "participant.export",
                &json!({"participant_id":session.principal().participant_id}),
            )
            .await?;
        let key = removal.cleanup.ok_or("cleanup missing")?;
        if ready {
            let authority = store
                .authorize_attendee_cleanup(session.session_fingerprint())
                .await?;
            let stopped = store
                .load_attendee_cleanup(&authority)
                .await?
                .ok_or("stop missing")?;
            store
                .record_attendee_cleanup(
                    &authority,
                    &AttendeeCleanupReport {
                        request_id: Uuid::new_v4(),
                        stopped,
                    },
                )
                .await?;
        } else {
            assert!(store.finish_room_runtime_cleanup(&key).await?.is_some());
        }
        let snapshot = store.snapshot("general", 0, 200).await?;
        let public = snapshot.agent_sessions.first().ok_or("session missing")?;
        assert_eq!(public.runtime_status, AgentRuntimeStatus::Stopped);
        assert!(!public.provider_session_active);
        assert!(
            snapshot
                .participants
                .iter()
                .any(|participant| participant.participant_id == key.session_id
                    && participant.status == ParticipantStatus::Exported)
        );
    }
    Ok(())
}
