use agentsassemble_domain::ParticipantRole;
use chrono::Utc;
use serde_json::json;

use super::{
    AGENT_ID, SECOND_AGENT_ID, attached_session, fixture, insert_agent, participant, stored_session,
};

async fn insert_waiting_agent(store: &crate::SqliteStore) {
    let now = Utc::now();
    let participant = participant(
        SECOND_AGENT_ID,
        "Flash",
        "agent",
        ParticipantRole::Agent,
        now,
    );
    let mut session = attached_session(now);
    SECOND_AGENT_ID.clone_into(&mut session.public.session_id);
    SECOND_AGENT_ID.clone_into(&mut session.public.participant_id);
    "Flash".clone_into(&mut session.public.display_name);
    "provider-thread-2".clone_into(&mut session.provider_session_id);
    "owned-runtime-2".clone_into(&mut session.runtime_handle_id);
    insert_agent(store, &participant, &session).await;
}

async fn count_events_containing(store: &crate::SqliteStore, text: &str) -> i64 {
    sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events WHERE instr(event_json, ?) > 0")
        .bind(text)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count matching events: {error}"))
}

#[tokio::test]
async fn runtime_gone_checkpoint_yields_to_an_inflight_stop_owner() {
    let (store, principal, _directory) = fixture().await;
    insert_waiting_agent(&store).await;

    let active = store
        .execute_message_with_turn(
            &principal,
            "stop-gone-active",
            "message.send",
            &json!({"content": "@Terra hold the floor during stop"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign stop-race turn: {error}"));
    let assignment = &active.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize stop-race turn: {error}"));
    store
        .mark_provider_turn_running(&start, "provider-turn-stop-race")
        .await
        .unwrap_or_else(|error| panic!("mark stop-race turn running: {error}"));
    let stale_candidate = store
        .load_active_provider_turn_reconciliation_candidate("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load provider candidate before stop: {error}"))
        .unwrap_or_else(|| panic!("missing provider candidate before stop"));

    let waiting = store
        .execute_message_with_turn(
            &principal,
            "stop-gone-waiting",
            "message.send",
            &json!({"content": "@Flash wait behind the stop owner"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue behind stop owner: {error}"));
    assert!(waiting.assignments.is_empty());

    let payload = json!({"agent_id": AGENT_ID});
    let crate::AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "stop-gone-owner", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop-race owner: {error}"))
    else {
        panic!("busy runtime must require an exact stop effect");
    };
    store
        .authorize_agent_stop_effect(&principal, "stop-gone-owner", &payload, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize stop-race owner: {error}"));

    let checkpoint = store
        .finalize_provider_turn_runtime_gone(&stale_candidate)
        .await
        .unwrap_or_else(|error| panic!("checkpoint stop-confirmed runtime Gone: {error}"));
    assert!(checkpoint.events.is_empty());
    assert!(checkpoint.next_assignments.is_empty());
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("load stop-owned execution: {error}"));
    assert_eq!(
        execution.phase,
        crate::ProviderTurnExecutionPhase::Interrupted
    );
    assert!(execution.requeue_finalized);
    let checkpointed_session = stored_session(&store).await;
    assert_eq!(checkpointed_session.lifecycle_intent_action, "stop");
    assert_eq!(
        checkpointed_session.lifecycle_intent_status,
        "effect_applied"
    );
    assert_eq!(
        checkpointed_session.public.active_turn_id,
        assignment.turn_id
    );
    assert_eq!(
        count_events_containing(&store, "provider_runtime_gone").await,
        0
    );
    assert_eq!(count_events_containing(&store, "operator_stop").await, 1);

    store
        .record_agent_stop_effect("general", AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("replay checkpointed stop confirmation: {error}"));
    let finalized = store
        .finalize_agent_stop(&principal, "stop-gone-owner", &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize stop-race owner: {error}"));
    assert_eq!(finalized.assignments.len(), 1);
    assert_eq!(
        finalized.assignments[0].session.public.session_id,
        SECOND_AGENT_ID
    );
}

#[tokio::test]
async fn runtime_gone_rejects_a_stop_intent_without_its_exact_reservation() {
    let (store, principal, _directory) = fixture().await;
    let active = store
        .execute_message_with_turn(
            &principal,
            "forged-stop-active",
            "message.send",
            &json!({"content": "@Terra retain authority on invalid stop"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign forged-stop turn: {error}"));
    let assignment = &active.assignments[0];
    let start = store
        .authorize_provider_turn_start(
            "general",
            AGENT_ID,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize forged-stop turn: {error}"));
    store
        .mark_provider_turn_running(&start, "provider-turn-forged-stop")
        .await
        .unwrap_or_else(|error| panic!("mark forged-stop turn running: {error}"));
    let candidate = store
        .load_active_provider_turn_reconciliation_candidate("general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load forged-stop provider candidate: {error}"))
        .unwrap_or_else(|| panic!("missing forged-stop provider candidate"));

    let payload = json!({"agent_id": AGENT_ID});
    let crate::AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "forged-stop-owner", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare forged-stop owner: {error}"))
    else {
        panic!("busy runtime must require an exact stop effect");
    };
    store
        .authorize_agent_stop_effect(
            &principal,
            "forged-stop-owner",
            &payload,
            &stop.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize forged-stop owner: {error}"));
    sqlx::query(
        "DELETE FROM lifecycle_command_reservations WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("remove forged-stop reservation fixture: {error}"));

    let result = store.finalize_provider_turn_runtime_gone(&candidate).await;
    assert!(matches!(
        result,
        Err(crate::PersistenceError::CommandRejected {
            code: "invalid_stored_runtime_authority",
            ..
        })
    ));
    let execution = store
        .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("reload forged-stop execution: {error}"));
    assert_eq!(execution.phase, crate::ProviderTurnExecutionPhase::Running);
    assert!(!execution.requeue_finalized);
    let unchanged = stored_session(&store).await;
    assert_eq!(unchanged.lifecycle_intent_status, "effect_inflight");
    assert_eq!(count_events_containing(&store, "operator_stop").await, 0);
}
