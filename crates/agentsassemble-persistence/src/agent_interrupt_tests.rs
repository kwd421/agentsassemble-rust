use serde_json::json;

use super::{AGENT_ID, event_types, fixture, input_ids, stored_session};
use crate::{PersistenceError, ProviderTurnExecutionPhase, ProviderTurnInterruptCause};

#[tokio::test]
async fn explicit_interrupt_is_exact_replayable_and_does_not_rerun_restored_input() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "interrupt-source",
            "message.send",
            &json!({"content": "@Terra retain this input after interrupt"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign interrupt turn: {error}"));
    let assignment = mutation.assignments[0].clone();
    let payload = json!({"agent_id": AGENT_ID});
    let accepted = store
        .execute_agent_interrupt(&principal, "interrupt-request", &payload)
        .await
        .unwrap_or_else(|error| panic!("accept exact interrupt: {error}"));
    assert_eq!(accepted.outcome.event.event_type, "agent_session_state");
    assert_eq!(accepted.outcome.result["interrupt_requested"], json!(true));
    let effect = accepted
        .interrupt_effect
        .unwrap_or_else(|| panic!("fresh command owns an interrupt effect"));
    assert_eq!(effect.cause, ProviderTurnInterruptCause::AgentInterrupt);
    assert_eq!(effect.execution_id, assignment.execution_id);
    assert_eq!(effect.turn_id, assignment.turn_id);

    let replay = store
        .execute_agent_interrupt(&principal, "interrupt-request", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay accepted interrupt: {error}"));
    assert!(replay.outcome.deduplicated);
    assert!(replay.interrupt_effect.is_none());
    assert_eq!(replay.outcome.result, accepted.outcome.result);
    assert!(matches!(
        store
            .execute_agent_interrupt(
                &principal,
                "interrupt-request",
                &json!({"agent_id": "another-agent"}),
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(matches!(
        store
            .execute_agent_interrupt(&principal, "second-interrupt", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "provider_turn_interrupt_in_progress",
            ..
        })
    ));

    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000301")
        .await
        .unwrap_or_else(|error| panic!("claim explicit interrupt: {error}"));
    let waiting = store
        .mark_unstarted_interrupt_waiting(&claim)
        .await
        .unwrap_or_else(|error| panic!("record pre-dispatch quiescence: {error}"));
    let committed = store
        .finalize_interrupted_turn_retained(&waiting)
        .await
        .unwrap_or_else(|error| panic!("finalize retained explicit interrupt: {error}"));
    assert_eq!(
        event_types(&committed.events),
        ["error", "turn_finished", "agent_session_state"]
    );
    assert_eq!(committed.events[0].extra["error_code"], "interrupted");
    assert_eq!(committed.events[1].extra["reason_code"], "agent_interrupt");
    assert!(committed.next_assignments.is_empty());

    let retained = stored_session(&store).await;
    assert_eq!(retained.public.status, "attached");
    assert_eq!(retained.public.runtime_status, "idle");
    assert!(retained.public.enabled);
    assert_eq!(
        retained.runtime_handle_id,
        assignment.session.runtime_handle_id
    );
    assert_eq!(
        retained.provider_session_id,
        assignment.session.provider_session_id
    );
    assert_eq!(
        input_ids(&retained.pending_inputs),
        input_ids(&assignment.session.inflight_inputs)
    );
    assert!(retained.inflight_inputs.is_empty());
    assert_eq!(retained.public.last_error_code, "interrupted");
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("read terminal interrupt execution: {error}"));
    assert_eq!(execution.phase, ProviderTurnExecutionPhase::Interrupted);
    assert!(execution.requeue_finalized);
}

#[tokio::test]
async fn runtime_gone_explicit_interrupt_restores_input_without_floor_progression() {
    let (store, principal, _directory) = fixture().await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "gone-interrupt-source",
            "message.send",
            &json!({"content": "@Terra retain after runtime loss"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign runtime-gone turn: {error}"));
    let assignment = mutation.assignments[0].clone();
    let accepted = store
        .execute_agent_interrupt(
            &principal,
            "gone-interrupt-request",
            &json!({"agent_id": AGENT_ID}),
        )
        .await
        .unwrap_or_else(|error| panic!("accept runtime-gone interrupt: {error}"));
    let effect = accepted
        .interrupt_effect
        .unwrap_or_else(|| panic!("runtime-gone effect"));
    let candidate = store
        .load_provider_turn_reconciliation_candidate(
            "general",
            AGENT_ID,
            assignment.turn_generation,
        )
        .await
        .unwrap_or_else(|error| panic!("load runtime-gone candidate: {error}"));
    assert_eq!(candidate.effect, Some(effect));
    let committed = store
        .finalize_provider_turn_runtime_gone(&candidate)
        .await
        .unwrap_or_else(|error| panic!("finalize runtime-gone interrupt: {error}"));
    assert_eq!(
        event_types(&committed.events),
        ["error", "turn_finished", "agent_session_state"]
    );
    assert_eq!(committed.events[1].extra["reason_code"], "agent_interrupt");
    assert!(committed.next_assignments.is_empty());

    let detached = stored_session(&store).await;
    assert_eq!(detached.public.status, "detached");
    assert_eq!(detached.public.runtime_status, "stopped");
    assert!(!detached.public.enabled);
    assert!(detached.runtime_handle_id.is_empty());
    assert_eq!(
        input_ids(&detached.pending_inputs),
        input_ids(&assignment.session.inflight_inputs)
    );
    assert_eq!(detached.public.last_error_code, "interrupted");
}
