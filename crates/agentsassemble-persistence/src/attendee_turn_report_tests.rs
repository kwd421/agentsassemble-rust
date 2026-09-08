use crate::{
    AttendeeConnectionAuthorization, AttendeeTurnOutcome, AttendeeTurnReport, PersistenceError,
    RoomMutationAuthority, SqliteStore, attendee_connection_tests::fixture,
    attendee_ready_tests::report as ready_report,
    human_session_authority_tests::session_fingerprint,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_report_replacement_and_response_loss_preserve_one_canonical_commit() -> TestResult
{
    let (store, first, report, now) = assigned_report().await?;
    let replacement = store
        .claim_attendee_connection(first.session(), Uuid::new_v4(), now)
        .await?
        .authorization;
    assert!(matches!(
        store
            .record_attendee_turn_report(&first, &report, now)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "bridge_connection_replaced",
            ..
        })
    ));
    store
        .record_attendee_ready(&replacement, &ready_report(), now)
        .await?;
    let mut wrong = report.clone();
    wrong.start_dispatch_nonce = "wrong-dispatch".to_owned();
    assert!(
        store
            .record_attendee_turn_report(&replacement, &wrong, now)
            .await
            .is_err()
    );
    let (one, two) = tokio::join!(
        store.record_attendee_turn_report(&replacement, &report, now),
        store.record_attendee_turn_report(&replacement, &report, now),
    );
    let one = one?;
    let two = two?;
    assert_ne!(one.outcome.deduplicated, two.outcome.deduplicated);
    assert_eq!(one.outcome.event.id, two.outcome.event.id);
    assert_eq!(
        one.outcome.event.content.as_deref(),
        Some("Exact external result")
    );
    assert!(two.assignments.is_empty());
    let serialized = serde_json::to_string(&one.outcome.result)?;
    assert!(!serialized.contains(&report.runtime_lease_token));
    assert!(!serialized.contains(&report.start_dispatch_nonce));
    let snapshot = store.snapshot("general", 0, 200).await?;
    assert_eq!(
        snapshot
            .events
            .iter()
            .filter(|event| event.content.as_deref() == Some("Exact external result"))
            .count(),
        1
    );
    wrong = report.clone();
    wrong.outcome = AttendeeTurnOutcome::Message {
        content: "Changed result".to_owned(),
        target_agent_id: String::new(),
    };
    assert!(matches!(
        store
            .record_attendee_turn_report(&replacement, &wrong, now)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    // Even a committed receipt requires live provenance, including parent authority.
    sqlx::query("UPDATE room_attendee_invites SET revoked=1 WHERE session_fingerprint=?")
        .bind(replacement.session().session_fingerprint().as_slice())
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .record_attendee_turn_report(&replacement, &report, now)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn attendee_reports_share_vote_decline_and_failure_terminal_owners() -> TestResult {
    for (outcome, expected_phase) in [
        (
            AttendeeTurnOutcome::Vote {
                payload: json!({"kind":"vote", "vote_question":"Choose one", "vote_options":["A", "B"]}),
            },
            "completed",
        ),
        (
            AttendeeTurnOutcome::Declined {
                reason_code: "not_addressed".to_owned(),
            },
            "declined",
        ),
        (
            AttendeeTurnOutcome::Failed {
                error_code: "provider_failed".to_owned(),
            },
            "failed",
        ),
    ] {
        let (store, connection, mut report, now) = assigned_report().await?;
        report.outcome = outcome;
        let committed = store
            .record_attendee_turn_report(&connection, &report, now)
            .await?;
        assert!(!committed.outcome.events.is_empty());
        let phase: String =
            sqlx::query_scalar("SELECT phase FROM provider_turn_executions WHERE execution_id=?")
                .bind(&report.execution_id)
                .fetch_one(&store.pool)
                .await?;
        assert_eq!(phase, expected_phase);
        let retried = store
            .record_attendee_turn_report(&connection, &report, now)
            .await?;
        assert!(retried.outcome.deduplicated);
        assert_eq!(committed.outcome.event.id, retried.outcome.event.id);
        if expected_phase == "failed" {
            let public = store
                .snapshot("general", 0, 200)
                .await?
                .agent_sessions
                .pop()
                .ok_or("session missing")?;
            assert!(
                public.provider_session_active,
                "A failure report does not confirm external process cleanup"
            );
            assert!(public.recovery_required);
        }
    }
    Ok(())
}

async fn assigned_report() -> Result<
    (
        SqliteStore,
        AttendeeConnectionAuthorization,
        AttendeeTurnReport,
        DateTime<Utc>,
    ),
    Box<dyn std::error::Error>,
> {
    let (store, session, now) = fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &ready_report(), now)
        .await?;
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let sent = store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"External report contract"}),
        )
        .await?;
    let assigned = sent.assignments.first().ok_or("assignment missing")?;
    let start = store
        .authorize_provider_turn_start(
            "general",
            &assigned.session.public.session_id,
            assigned.turn_generation,
            &assigned.turn_id,
        )
        .await?;
    store
        .mark_provider_turn_running(&start, "external-provider-turn")
        .await?;
    let report = AttendeeTurnReport {
        request_id: Uuid::new_v4(),
        turn_id: start.turn_id,
        turn_generation: start.turn_generation,
        execution_id: start.execution_id,
        start_dispatch_nonce: start.start_dispatch_nonce,
        runtime_handle_id: start.runtime_handle_id,
        runtime_owner_id: start.runtime_owner_id,
        runtime_lease_token: start.runtime_lease_token,
        provider_turn_id: "external-provider-turn".to_owned(),
        provider_session_id: Some("external-session".to_owned()),
        outcome: AttendeeTurnOutcome::Message {
            content: "Exact external result".to_owned(),
            target_agent_id: String::new(),
        },
    };
    Ok((store, connection, report, now))
}
