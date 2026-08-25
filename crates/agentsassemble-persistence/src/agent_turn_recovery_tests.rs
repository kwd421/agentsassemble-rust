use agentsassemble_domain::{DurableAgentSession, Participant, ParticipantStatus};
use serde_json::json;

use super::{AgentRuntimeStarted, AgentStartPlan, tests::fixture};
use crate::RuntimeReconciliationObservation;

#[tokio::test]
#[allow(clippy::too_many_lines)] // One restart scenario spans durable setup, adoption, and both recovered projections.
async fn adopted_runtime_requeues_an_active_turn_instead_of_leaving_it_stuck() {
    let (store, principal, _directory) = fixture().await;
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(super::tests::AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load recovery fixture session: {error}"));
    let mut clean_session: DurableAgentSession = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode recovery fixture session: {error}"));
    clean_session.pending_inputs.clear();
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(&clean_session)
            .unwrap_or_else(|error| panic!("encode recovery fixture session: {error}")),
    )
    .bind(super::tests::AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("clean recovery fixture queue: {error}"));
    let payload = json!({"agent_id": super::tests::AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "turn-recovery-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare recovery start: {error}"))
    else {
        panic!("stopped recovery fixture must require a start effect");
    };
    store
        .authorize_agent_start_effect(
            &principal,
            "turn-recovery-start",
            &payload,
            &start.operation_id,
            "agent.start",
            "adopted-runtime",
            "previous-supervisor",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize recovery start: {error}"));
    store
        .complete_agent_start(
            &principal,
            "turn-recovery-start",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "adopted-runtime".to_owned(),
                runtime_owner_id: "previous-supervisor".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
                provider_session_id: "provider-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete recovery start: {error}"));
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "turn-before-restart",
            "message.send",
            &json!({"content": "@Terra survive supervisor restart"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign turn before restart: {error}"));
    let assignment = mutation
        .assignments
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("message must have an active turn before restart"));
    let candidate = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load active-turn reconciliation candidate: {error}"))
        .into_iter()
        .find(|candidate| candidate.session.public.session_id == super::tests::AGENT_ID)
        .unwrap_or_else(|| panic!("active turn had no reconciliation candidate"));
    store
        .apply_runtime_reconciliation(
            &candidate,
            &RuntimeReconciliationObservation::Adopted {
                handle_id: "adopted-runtime".to_owned(),
                previous_owner_id: "previous-supervisor".to_owned(),
                new_owner_id: "new-supervisor".to_owned(),
                runtime_profile_key: candidate.session.runtime_profile_key.clone(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("apply active-turn adoption: {error}"));

    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(super::tests::AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load reconciled session: {error}"));
    let reconciled: DurableAgentSession = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode reconciled session: {error}"));
    assert_eq!(
        reconciled.pending_inputs,
        assignment.session.inflight_inputs
    );
    assert!(reconciled.inflight_inputs.is_empty());
    assert!(reconciled.public.active_turn_id.is_empty());
    assert!(reconciled.active_source_event_id.is_empty());
    assert_eq!(reconciled.public.runtime_status, "recovering");
    assert_eq!(reconciled.public.status, "unavailable");
    assert!(!reconciled.public.enabled);
    assert!(!reconciled.public.provider_session_active);
    assert!(reconciled.public.recovery_required);
    assert_eq!(
        reconciled.public.last_error_code,
        "provider_turn_recovery_required"
    );
    assert_eq!(reconciled.runtime_owner_id, "new-supervisor");

    let participant = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(super::tests::AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load reconciled participant: {error}"));
    let participant: Participant = serde_json::from_str(&participant)
        .unwrap_or_else(|error| panic!("decode reconciled participant: {error}"));
    assert_eq!(participant.status, ParticipantStatus::Detached);
}
