use crate::{
    AttendeeRuntimeReady, PersistenceError, RoomMutationAuthority,
    attendee_connection_tests::fixture, human_session_authority_tests::session_fingerprint,
};
use agentsassemble_domain::AgentRuntimeStatus;
use serde_json::json;
use uuid::Uuid;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn attendee_ready_reconnect_retains_exact_running_turn_and_external_custody() -> TestResult {
    let (store, session, now) = fixture().await?;
    let first = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    let ready = report();
    let initial = store.record_attendee_ready(&first, &ready, now).await?;
    assert_eq!(initial.events.len(), 1);
    assert!(
        store
            .record_attendee_ready(&first, &ready, now)
            .await?
            .events
            .is_empty()
    );
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let sent = store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"External turn proof"}),
        )
        .await?;
    assert_eq!(sent.assignments.len(), 1);
    let assigned = &sent.assignments[0];
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
    let before = store
        .load_active_provider_turn_reconciliation_candidate(
            "general",
            &session.principal().participant_id,
        )
        .await?
        .ok_or("active turn missing")?;
    assert!(
        store
            .load_provider_turn_reconciliation_page(None)
            .await?
            .candidates
            .is_empty()
    );
    let replacement = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    assert!(
        store
            .record_attendee_ready(&first, &ready, now)
            .await
            .is_err()
    );
    assert!(
        store
            .disconnect_attendee_connection(&first, now)
            .await?
            .is_none()
    );
    let mut wrong = ready.clone();
    wrong.runtime_lease_token = "different-generation".to_owned();
    assert!(matches!(
        store.record_attendee_ready(&replacement, &wrong, now).await,
        Err(PersistenceError::CommandRejected {
            code: "runtime_owner_mismatch",
            ..
        })
    ));
    store
        .record_attendee_ready(&replacement, &ready, now)
        .await?;
    let disconnected = store
        .disconnect_attendee_connection(&replacement, now)
        .await?
        .ok_or("disconnect event missing")?;
    assert_eq!(
        disconnected.extra["agent_session"]["runtime_status"],
        "busy"
    );
    verify_retained_turn(&store, &session, &before).await
}

#[tokio::test]
async fn attendee_idle_disconnect_requires_ready_before_assigning_queued_observation() -> TestResult
{
    let (store, session, now) = fixture().await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &report(), now)
        .await?;
    store.disconnect_attendees_before_admission().await?;
    let public = store
        .snapshot("general", 0, 200)
        .await?
        .agent_sessions
        .pop()
        .ok_or("session missing")?;
    assert_eq!(public.runtime_status, AgentRuntimeStatus::Disconnected);
    assert!(public.provider_session_active);
    let human = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await?;
    let sent = store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"@Companion queued mention"}),
        )
        .await?;
    assert!(sent.assignments.is_empty());
    let replacement = store
        .claim_attendee_connection(&session, Uuid::new_v4(), now)
        .await?
        .authorization;
    assert!(store.assign_pending_turn("general").await?.is_none());
    let resumed = store
        .record_attendee_ready(&replacement, &report(), now)
        .await?;
    assert_eq!(resumed.next_assignments.len(), 1);
    Ok(())
}

fn report() -> AttendeeRuntimeReady {
    AttendeeRuntimeReady {
        runtime_handle_id: "external-runtime".to_owned(),
        runtime_owner_id: "external-owner".to_owned(),
        runtime_lease_token: "external-generation".to_owned(),
        provider_session_id: "external-session".to_owned(),
        model: "local-contract-model".to_owned(),
        reasoning_effort: "high".to_owned(),
        service_tier: String::new(),
        variant: String::new(),
        execution_harness: "builtin".to_owned(),
        permission_mode: "meeting_read_only".to_owned(),
        max_output_tokens: 0,
    }
}

async fn verify_retained_turn(
    store: &crate::SqliteStore,
    session: &crate::AttendeeSessionAuthorization,
    before: &crate::ProviderTurnReconciliationCandidate,
) -> TestResult {
    let after = store
        .load_active_provider_turn_reconciliation_candidate(
            "general",
            &session.principal().participant_id,
        )
        .await?
        .ok_or("active turn lost")?;
    assert_eq!(before.execution, after.execution);
    assert_eq!(
        before.session.public.active_turn_id,
        after.session.public.active_turn_id
    );
    assert_eq!(
        after.session.public.runtime_status,
        AgentRuntimeStatus::Busy
    );
    assert!(after.session.public.provider_session_active);
    assert!(store.assign_pending_turn("general").await?.is_none());
    Ok(())
}
