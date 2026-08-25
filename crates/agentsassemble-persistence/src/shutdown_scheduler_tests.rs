use agentsassemble_domain::{DurableAgentSession, ParticipantRole};
use chrono::Utc;
use serde_json::json;

use super::{AGENT_ID, SECOND_AGENT_ID, attached_session, fixture, insert_agent, participant};

#[tokio::test]
async fn shutdown_never_assigns_work_to_an_already_stopped_runtime_in_either_order() {
    Box::pin(assert_shutdown_order(false)).await;
    Box::pin(assert_shutdown_order(true)).await;
}

async fn assert_shutdown_order(stop_waiting_agent_first: bool) {
    let (store, principal, _directory) = fixture().await;
    let now = Utc::now();
    let flash_participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut flash_session = attached_session(now);
    SECOND_AGENT_ID.clone_into(&mut flash_session.public.session_id);
    SECOND_AGENT_ID.clone_into(&mut flash_session.public.participant_id);
    "Flash".clone_into(&mut flash_session.public.display_name);
    "provider-thread-2".clone_into(&mut flash_session.provider_session_id);
    "owned-runtime-2".clone_into(&mut flash_session.runtime_handle_id);
    insert_agent(&store, &flash_participant, &flash_session).await;

    let active = store
        .execute_message_with_turn(
            &principal,
            "shutdown-active-floor",
            "message.send",
            &json!({"content": "@Terra hold the floor during shutdown"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign shutdown floor: {error}"));
    assert_eq!(active.assignments.len(), 1);
    let waiting = store
        .execute_message_with_turn(
            &principal,
            "shutdown-waiting-floor",
            "message.send",
            &json!({"content": "@Flash remain pending during shutdown"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue shutdown floor: {error}"));
    assert!(waiting.assignments.is_empty());

    let terra = store
        .load_active_provider_turn_reconciliation_candidate("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load shutdown turn candidate: {error}"))
        .unwrap_or_else(|| panic!("shutdown turn candidate is missing"));
    let flash = store
        .load_runtime_reconciliation_candidate("general", SECOND_AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load shutdown runtime candidate: {error}"))
        .unwrap_or_else(|| panic!("shutdown runtime candidate is missing"));

    if stop_waiting_agent_first {
        store
            .apply_runtime_shutdown_reconciliation(
                &flash,
                &crate::RuntimeReconciliationObservation::Gone,
            )
            .await
            .unwrap_or_else(|error| panic!("checkpoint waiting runtime first: {error}"));
        store
            .finalize_provider_turn_runtime_gone_for_shutdown(&terra)
            .await
            .unwrap_or_else(|error| panic!("checkpoint floor runtime second: {error}"));
    } else {
        store
            .finalize_provider_turn_runtime_gone_for_shutdown(&terra)
            .await
            .unwrap_or_else(|error| panic!("checkpoint floor runtime first: {error}"));
        store
            .apply_runtime_shutdown_reconciliation(
                &flash,
                &crate::RuntimeReconciliationObservation::Gone,
            )
            .await
            .unwrap_or_else(|error| panic!("checkpoint waiting runtime second: {error}"));
    }

    let nonterminal = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM provider_turn_executions WHERE room_id = 'general' AND phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count shutdown executions: {error}"));
    assert_eq!(nonterminal, 0);
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(SECOND_AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load shutdown waiting session: {error}"));
    let waiting_session = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode shutdown waiting session: {error}"));
    assert_eq!(waiting_session.pending_inputs.len(), 1);
    assert!(waiting_session.inflight_inputs.is_empty());
}
