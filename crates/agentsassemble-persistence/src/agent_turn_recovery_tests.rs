use agentsassemble_domain::DurableAgentSession;
use serde_json::json;

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan, SqliteStore, tests::fixture};
use crate::ProviderTurnExecutionPhase;

async fn clear_fixture_queue(store: &SqliteStore) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(super::tests::AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load recovery fixture session: {error}"));
    let mut session: DurableAgentSession = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode recovery fixture session: {error}"));
    session.pending_inputs.clear();
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(&session)
            .unwrap_or_else(|error| panic!("encode recovery fixture session: {error}")),
    )
    .bind(super::tests::AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("clean recovery fixture queue: {error}"));
}

#[tokio::test]
async fn blocking_provider_execution_owns_restart_before_lifecycle_reconciliation() {
    let (store, principal, _directory) = fixture().await;
    clear_fixture_queue(&store).await;
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
    assert!(
        store
            .load_runtime_reconciliation_candidates()
            .await
            .unwrap_or_else(|error| panic!("load active-turn reconciliation candidate: {error}"))
            .is_empty(),
        "lifecycle recovery must not clear authority owned by a blocking provider execution"
    );
    let page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("load provider-turn reconciliation candidate: {error}"));
    assert_eq!(page.candidates.len(), 1);
    let recovered = store
        .recover_assigned_provider_turn(&page.candidates[0])
        .await
        .unwrap_or_else(|error| panic!("recover exact pre-dispatch assignment: {error}"));
    assert_eq!(recovered.turn_id, assignment.turn_id);
    assert_eq!(recovered.turn_generation, assignment.turn_generation);
    assert_eq!(recovered.execution_id, assignment.execution_id);
    assert_eq!(recovered.provider_input, assignment.provider_input);
    assert_eq!(recovered.room_view, assignment.room_view);
    assert_eq!(
        recovered.session.inflight_inputs,
        assignment.session.inflight_inputs
    );
    assert_eq!(recovered.session.runtime_owner_id, "previous-supervisor");

    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "stop-blocking-turn", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare blocking-turn stop: {error}"))
    else {
        panic!("active runtime must require an exact stop effect");
    };
    store
        .authorize_agent_stop_effect(
            &principal,
            "stop-blocking-turn",
            &payload,
            &stop.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize blocking-turn stop: {error}"));
    store
        .record_agent_stop_effect("general", super::tests::AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("checkpoint confirmed blocking-turn stop: {error}"));
    let execution = store
        .provider_turn_execution(
            "general",
            super::tests::AGENT_ID,
            assignment.turn_generation,
        )
        .await
        .unwrap_or_else(|error| panic!("read stopped provider execution: {error}"));
    assert_eq!(execution.phase, ProviderTurnExecutionPhase::Interrupted);
    assert!(execution.requeue_finalized);
    assert!(
        store
            .load_provider_turn_reconciliation_page(None)
            .await
            .unwrap_or_else(|error| panic!("scan after confirmed stop: {error}"))
            .candidates
            .is_empty()
    );
    store
        .finalize_agent_stop(&principal, "stop-blocking-turn", &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize blocking-turn stop: {error}"));
}

#[tokio::test]
async fn ambiguous_stop_preserves_blocking_turn_authority_for_provider_reconciliation() {
    let (store, principal, _directory) = fixture().await;
    clear_fixture_queue(&store).await;
    let payload = json!({"agent_id": super::tests::AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "ambiguous-turn-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare ambiguous-turn start: {error}"))
    else {
        panic!("stopped fixture must require a start effect");
    };
    store
        .authorize_agent_start_effect(
            &principal,
            "ambiguous-turn-start",
            &payload,
            &start.operation_id,
            "agent.start",
            "ambiguous-runtime",
            "ambiguous-owner",
            "ambiguous-lease",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize ambiguous runtime: {error}"));
    store
        .complete_agent_start(
            &principal,
            "ambiguous-turn-start",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "ambiguous-runtime".to_owned(),
                runtime_owner_id: "ambiguous-owner".to_owned(),
                runtime_lease_token: "ambiguous-lease".to_owned(),
                provider_session_id: "ambiguous-provider-session".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete ambiguous runtime: {error}"));
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "ambiguous-stop-turn",
            "message.send",
            &json!({"content": "@Terra preserve ambiguous stop custody"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign ambiguous stop turn: {error}"));
    let assignment = &mutation.assignments[0];
    store
        .authorize_provider_turn_start(
            "general",
            super::tests::AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize ambiguous provider turn: {error}"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "ambiguous-busy-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare ambiguous busy stop: {error}"))
    else {
        panic!("busy runtime must require stop effect");
    };
    store
        .authorize_agent_stop_effect(
            &principal,
            "ambiguous-busy-stop",
            &payload,
            &stop.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize ambiguous busy stop: {error}"));
    store
        .mark_agent_stop_unconfirmed(
            &principal,
            super::tests::AGENT_ID,
            &stop.operation_id,
            "runtime_stop_unconfirmed",
            "runtime stop boundary was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("record ambiguous busy stop: {error}"));

    let page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("load ambiguous provider candidate: {error}"));
    assert_eq!(page.candidates.len(), 1);
    assert_eq!(
        page.candidates[0].execution.execution_id,
        assignment.execution_id
    );
    assert_eq!(
        page.candidates[0].execution.phase,
        ProviderTurnExecutionPhase::StartDispatching
    );
    assert!(
        store
            .load_runtime_reconciliation_candidates()
            .await
            .unwrap_or_else(|error| panic!("scan lifecycle behind provider custody: {error}"))
            .is_empty()
    );
}
