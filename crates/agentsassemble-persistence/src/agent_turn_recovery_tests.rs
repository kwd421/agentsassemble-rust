use agentsassemble_domain::{AuthenticatedPrincipal, DurableAgentSession};
use serde_json::{Value, json};

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

async fn start_fixture_runtime(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    handle_id: &str,
    owner_id: &str,
    lease_token: &str,
    provider_session_id: &str,
) -> Value {
    let payload = json!({"agent_id": super::tests::AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(principal, request_id, &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare recovery start: {error}"))
    else {
        panic!("stopped recovery fixture must require a start effect");
    };
    store
        .authorize_agent_start_effect(
            principal,
            request_id,
            &payload,
            &start.operation_id,
            "agent.start",
            handle_id,
            owner_id,
            lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize recovery start: {error}"));
    store
        .complete_agent_start(
            principal,
            request_id,
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: handle_id.to_owned(),
                runtime_owner_id: owner_id.to_owned(),
                runtime_lease_token: lease_token.to_owned(),
                provider_session_id: provider_session_id.to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete recovery start: {error}"));
    payload
}

#[tokio::test]
async fn blocking_provider_execution_owns_restart_before_lifecycle_reconciliation() {
    let (store, principal, _directory) = fixture().await;
    clear_fixture_queue(&store).await;
    let payload = start_fixture_runtime(
        &store,
        &principal,
        "turn-recovery-start",
        "adopted-runtime",
        "previous-supervisor",
        "lease-generation-1",
        "provider-thread",
    )
    .await;
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
    let payload = start_fixture_runtime(
        &store,
        &principal,
        "ambiguous-turn-start",
        "ambiguous-runtime",
        "ambiguous-owner",
        "ambiguous-lease",
        "ambiguous-provider-session",
    )
    .await;
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

#[tokio::test]
async fn retained_no_io_proof_terminalizes_an_ambiguous_start_authorization() {
    let (store, principal, _directory) = fixture().await;
    clear_fixture_queue(&store).await;
    start_fixture_runtime(
        &store,
        &principal,
        "no-io-turn-start",
        "no-io-runtime",
        "no-io-owner",
        "no-io-lease",
        "no-io-provider-session",
    )
    .await;
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "no-io-turn",
            "message.send",
            &json!({"content": "@Terra retain pre-dispatch proof"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign no-I/O turn: {error}"));
    let assignment = &mutation.assignments[0];
    store
        .authorize_provider_turn_start(
            "general",
            super::tests::AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize no-I/O turn: {error}"));
    let candidate = store
        .load_active_provider_turn_reconciliation_candidate("general", super::tests::AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load no-I/O candidate: {error}"))
        .unwrap_or_else(|| panic!("missing no-I/O candidate"));

    let commit = store
        .finalize_provider_turn_not_started(&candidate)
        .await
        .unwrap_or_else(|error| panic!("terminalize no-I/O turn: {error}"));
    assert_eq!(commit.events.len(), 3);
    let execution = store
        .provider_turn_execution(
            "general",
            super::tests::AGENT_ID,
            assignment.turn_generation,
        )
        .await
        .unwrap_or_else(|error| panic!("read no-I/O execution: {error}"));
    assert_eq!(execution.phase, ProviderTurnExecutionPhase::Failed);
    assert!(execution.requeue_finalized);
    assert!(
        store
            .load_active_provider_turn_reconciliation_candidate("general", super::tests::AGENT_ID,)
            .await
            .unwrap_or_else(|error| panic!("rescan no-I/O turn: {error}"))
            .is_none()
    );
}
