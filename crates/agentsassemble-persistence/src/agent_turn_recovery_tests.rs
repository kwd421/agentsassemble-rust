use agentsassemble_domain::DurableAgentSession;
use serde_json::json;

use super::{AgentRuntimeStarted, AgentStartPlan, SqliteStore, tests::fixture};

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
}
