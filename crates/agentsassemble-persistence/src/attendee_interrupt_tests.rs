use agentsassemble_domain::AgentRuntimeStatus;
use serde_json::json;
use uuid::Uuid;

use crate::{
    AttendeeInterruptReport, AttendeeInterruptedRuntime, PersistenceError,
    RoomMutationAuthority::TrustedPrincipal, attendee_turn_report_tests::assigned_report,
    human_session_authority_tests::local_operator_principal,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn exact_external_interrupt_reconnect_and_report_preserve_canonical_quiescence() -> TestResult
{
    for (muted, runtime) in [
        (true, AttendeeInterruptedRuntime::Retained),
        (false, AttendeeInterruptedRuntime::Retained),
        (false, AttendeeInterruptedRuntime::Gone),
    ] {
        let (store, connection, turn, now) = assigned_report().await?;
        let operator = local_operator_principal();
        let agent_id = &connection.session().principal().participant_id;
        let authority = store
            .authorize_attendee_cleanup(connection.session().session_fingerprint())
            .await?;
        assert!(
            store
                .deliver_attendee_interrupt(&connection, now)
                .await?
                .is_none()
        );
        if muted {
            store
                .execute_participant_mute(
                    TrustedPrincipal(&operator),
                    "external-mute",
                    &json!({"participant_id":agent_id,"muted":true}),
                )
                .await?;
        } else {
            store
                .execute_agent_interrupt(
                    TrustedPrincipal(&operator),
                    "external-interrupt",
                    &json!({"agent_id":agent_id}),
                )
                .await?;
        }
        assert!(
            store
                .record_attendee_turn_report(&connection, &turn, now)
                .await
                .is_err()
        );
        let interrupted = store
            .deliver_attendee_interrupt(&connection, now)
            .await?
            .ok_or("interrupt missing")?;
        assert_eq!(interrupted.authority.execution_id, turn.execution_id);
        let replacement = store
            .claim_attendee_connection(connection.session(), Uuid::new_v4(), now)
            .await?
            .authorization;
        assert!(
            store
                .deliver_attendee_interrupt(&connection, now)
                .await
                .is_err()
        );
        assert!(
            store.deliver_attendee_interrupt(&replacement, now).await? == Some(interrupted.clone())
        );
        let report = AttendeeInterruptReport {
            request_id: Uuid::new_v4(),
            interrupted,
            runtime,
        };
        assert!(
            store
                .record_attendee_interrupt(&authority, connection.connection_id(), &report, now)
                .await
                .is_err()
        );
        let mut wrong = report.clone();
        wrong.interrupted.dispatch_nonce = Uuid::new_v4().to_string();
        assert!(
            store
                .record_attendee_interrupt(&authority, replacement.connection_id(), &wrong, now)
                .await
                .is_err()
        );
        let (one, two) = tokio::join!(
            store.record_attendee_interrupt(&authority, replacement.connection_id(), &report, now),
            store.record_attendee_interrupt(&authority, replacement.connection_id(), &report, now),
        );
        let one = one?;
        let two = two?;
        assert_ne!(one.outcome.deduplicated, two.outcome.deduplicated);
        assert_eq!(one.outcome.event.id, two.outcome.event.id);
        assert!(!serde_json::to_string(&one.outcome.result)?.contains(&turn.runtime_lease_token));
        assert_quiesced(&store, runtime).await?;
        assert!(matches!(
            store
                .record_attendee_interrupt(&authority, replacement.connection_id(), &wrong, now)
                .await,
            Err(PersistenceError::CommandConflict)
        ));
    }
    Ok(())
}

#[tokio::test]
async fn readiness_cannot_change_exact_runtime_interrupt_capability_on_reconnect() -> TestResult {
    let (store, connection, _, now) = assigned_report().await?;
    let replacement = store
        .claim_attendee_connection(connection.session(), Uuid::new_v4(), now)
        .await?
        .authorization;
    let mut ready = crate::attendee_ready_tests::report();
    ready.retained_interrupt = false;
    assert!(matches!(
        store.record_attendee_ready(&replacement, &ready, now).await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"runtime_owner_mismatch")
    ));
    Ok(())
}

async fn assert_quiesced(
    store: &crate::SqliteStore,
    runtime: AttendeeInterruptedRuntime,
) -> TestResult {
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.event_type == "turn_finished")
            .count(),
        1
    );
    let session = &snapshot.agent_sessions[0];
    match runtime {
        AttendeeInterruptedRuntime::Retained => assert!(session.provider_session_active),
        AttendeeInterruptedRuntime::Gone => {
            assert!(!session.provider_session_active);
            assert_eq!(session.runtime_status, AgentRuntimeStatus::Stopped);
        }
    }
    assert!(session.active_turn_id.is_empty());
    Ok(())
}

#[tokio::test]
async fn external_runtime_without_retained_interrupt_rejects_before_preparing_effect() -> TestResult
{
    let (store, session, now) = crate::attendee_connection_tests::fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    let mut ready = crate::attendee_ready_tests::report();
    ready.retained_interrupt = false;
    store
        .record_attendee_ready(&connection, &ready, now)
        .await?;
    let human = store
        .authorize_human_session(
            &crate::human_session_authority_tests::session_fingerprint(&store).await,
        )
        .await?;
    store
        .execute_authorized_message_with_turn(
            crate::RoomMutationAuthority::HumanSession(&human),
            "input-to-unsupported",
            "message.send",
            &json!({"content":"Unsupported interrupt"}),
        )
        .await?;
    let operator = local_operator_principal();
    let result = store
        .execute_agent_interrupt(
            TrustedPrincipal(&operator),
            "unsupported-interrupt",
            &json!({"agent_id":session.principal().participant_id}),
        )
        .await;
    assert!(matches!(
        result,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"provider_turn_interrupt_unsupported")
    ));
    assert!(
        store
            .deliver_attendee_interrupt(&connection, now)
            .await?
            .is_none()
    );
    assert!(
        store
            .deliver_attendee_turn(&connection, now)
            .await?
            .is_some()
    );
    Ok(())
}
