use agentsassemble_domain::{
    DurableAgentSession, Participant, QueuedRoomInput, RoomInputDeliveryKind,
};
use serde_json::{Value, json};

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan};
use crate::{
    PersistenceError, RuntimeReconciliationObservation, SqliteStore,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};

const SECOND_AGENT_ID: &str = "codex-00000000-0000-5000-8000-000000000002";

fn started(handle: &str, provider_session_id: &str) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: handle.to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        runtime_lease_token: "lease-generation-1".to_owned(),
        provider_session_id: provider_session_id.to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
}

#[tokio::test]
async fn oversized_turn_queue_fails_before_lifecycle_or_reconciliation_effects() {
    let (store, principal, _directory) = fixture().await;
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read session: {error}"));
    let mut session = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode session: {error}"));
    session.pending_inputs = (0..=crate::turn_queue::MAX_QUEUED_EVENT_IDS)
        .map(|index| QueuedRoomInput {
            event_id: format!("event-{index}"),
            delivery_kind: RoomInputDeliveryKind::OrderedObservation,
        })
        .collect();
    sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(
        serde_json::to_string(&session)
            .unwrap_or_else(|error| panic!("encode oversized session: {error}")),
    )
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("store oversized session: {error}"));
    let payload = json!({"agent_id": AGENT_ID});

    for result in [
        store
            .prepare_agent_start(&principal, "oversized-start", &payload)
            .await
            .map(|_| ()),
        store
            .prepare_agent_stop(&principal, "oversized-stop", &payload)
            .await
            .map(|_| ()),
        store
            .load_runtime_reconciliation_candidates()
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected {
                code: "stored_turn_authority_invalid" | "invalid_stored_runtime_authority",
                ..
            })
        ));
    }
}

#[tokio::test]
async fn live_looking_start_requires_supervisor_confirmation_before_success() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(first) = store
        .prepare_agent_start(&principal, "first-observed-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare first start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    authorize_start(
        &store,
        &principal,
        "first-observed-start",
        &payload,
        &first,
        "owned-runtime",
    )
    .await;
    store
        .complete_agent_start(
            &principal,
            "first-observed-start",
            &payload,
            &first.operation_id,
            &started("owned-runtime", "provider-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete first start: {error}"));
    let AgentStartPlan::Start(reconcile) = store
        .prepare_agent_start(&principal, "second-observed-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare observed reuse: {error}"))
    else {
        panic!("durable liveness alone must not commit reused start");
    };
    assert_eq!(reconcile.session.runtime_handle_id, "owned-runtime");
    assert_eq!(reconcile.session.runtime_owner_id, "supervisor-instance-1");
    assert_eq!(reconcile.session.public.runtime_status, "idle");
}

#[tokio::test]
async fn ambiguous_start_retains_its_exact_runtime_lease_and_blocks_replacement() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "ambiguous-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare ambiguous start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    authorize_start(
        &store,
        &principal,
        "ambiguous-start",
        &payload,
        &start,
        "uncertain-runtime",
    )
    .await;
    store
        .mark_agent_start_unconfirmed(
            &principal,
            AGENT_ID,
            &start.operation_id,
            "uncertain-runtime",
            "supervisor-instance-1",
            "runtime_start_unconfirmed",
            "Provider initialization was not confirmed.",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile ambiguous start: {error}")),
        1
    );
    let retained = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load retained start lease: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("ambiguous start lease was released"));
    assert_eq!(retained.session.runtime_handle_id, "uncertain-runtime");
    assert_eq!(retained.session.runtime_owner_id, "supervisor-instance-1");
    assert_eq!(retained.session.lifecycle_intent_action, "start");
    assert_eq!(retained.session.lifecycle_intent_status, "unconfirmed");
    store
        .apply_runtime_reconciliation(
            &retained,
            &RuntimeReconciliationObservation::LeaseUncertain {
                handle_id: "uncertain-runtime".to_owned(),
                owner_id: "supervisor-instance-1".to_owned(),
                reason_code: "runtime_health_unknown".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|error| panic!("retain exact uncertain lease: {error}"));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-start", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "operation_in_progress",
            ..
        })
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "ambiguous-start", &payload)
            .await,
        Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            ..
        })
    ));
    let unchanged = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("reload retained start lease: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("unresolved replay released the runtime lease"));
    assert_eq!(unchanged.session.runtime_handle_id, "uncertain-runtime");
    assert_eq!(unchanged.session.runtime_owner_id, "supervisor-instance-1");
    assert_eq!(unchanged.session.lifecycle_intent_status, "unconfirmed");
}

#[tokio::test]
async fn reconciliation_rejects_competing_pending_lifecycle_authority() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(_) = store
        .prepare_agent_start(&principal, "authoritative-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare authoritative start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    sqlx::query(
        "INSERT INTO lifecycle_command_reservations(room_id, principal_id, request_id, action, payload_hash, principal_json, payload_json, supervisor_generation, session_id, operation_id, status) VALUES ('general', 'operator', 'competing-start', 'agent.start', 'competing-hash', ?, ?, 'fixture-generation', ?, 'competing-operation', 'pending')",
    )
    .bind(serde_json::to_string(&principal).unwrap_or_else(|error| panic!("encode principal: {error}")))
    .bind(serde_json::to_string(&payload).unwrap_or_else(|error| panic!("encode payload: {error}")))
    .bind(AGENT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert competing reservation: {error}"));

    assert!(matches!(
        store.load_runtime_reconciliation_candidates().await,
        Err(PersistenceError::CommandRejected {
            code: "invalid_stored_runtime_authority",
            ..
        })
    ));
}

#[tokio::test]
async fn runtime_reconciliation_uses_exact_cas_and_gone_stop_finalizes_without_repeating_effect() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-before-observation", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    authorize_start(
        &store,
        &principal,
        "start-before-observation",
        &payload,
        &start,
        "runtime-before-observation",
    )
    .await;
    store
        .complete_agent_start(
            &principal,
            "start-before-observation",
            &payload,
            &start.operation_id,
            &started("runtime-before-observation", "provider-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    let stale_candidate = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load stale candidate: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("live runtime had no candidate"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "stop-after-observation", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require a stop effect");
    };
    store
        .authorize_agent_stop_effect(
            &principal,
            "stop-after-observation",
            &payload,
            &stop.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize observed stop: {error}"));
    assert!(matches!(
        store
            .apply_runtime_reconciliation(
                &stale_candidate,
                &RuntimeReconciliationObservation::Gone,
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "stale_reconciliation_candidate",
            ..
        })
    ));
    let current = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load stop candidate: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("prepared stop had no candidate"));
    store
        .apply_runtime_reconciliation(&current, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("apply gone observation: {error}"));
    let AgentStopPlan::Outcome(outcome) = store
        .prepare_agent_stop(&principal, "stop-after-observation", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay observed stop: {error}"))
    else {
        panic!("startup recovery must own the complete stop result");
    };
    assert_eq!(outcome.result["agent_session"]["runtime_status"], "stopped");
    assert_eq!(stop.runtime_handle_id, "runtime-before-observation");
}

async fn clone_agent(store: &SqliteStore, agent_id: &str) {
    let participant = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read source participant: {error}"));
    let mut participant: Participant = serde_json::from_str(&participant)
        .unwrap_or_else(|error| panic!("decode source participant: {error}"));
    participant.participant_id = agent_id.to_owned();
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', ?, ?)",
    )
    .bind(agent_id)
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode cloned participant: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert cloned participant: {error}"));

    let session = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read source session: {error}"));
    let mut session: DurableAgentSession = serde_json::from_str(&session)
        .unwrap_or_else(|error| panic!("decode source session: {error}"));
    session.public.session_id = agent_id.to_owned();
    session.public.participant_id = agent_id.to_owned();
    sqlx::query(
        "INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES ('general', ?, ?)",
    )
    .bind(agent_id)
    .bind(
        serde_json::to_string(&session)
            .unwrap_or_else(|error| panic!("encode cloned session: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert cloned session: {error}"));
}

#[tokio::test]
async fn pending_request_identity_cannot_be_rebound_to_another_agent() {
    let (store, principal, _directory) = fixture().await;
    clone_agent(&store, SECOND_AGENT_ID).await;
    let first_payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(first) = store
        .prepare_agent_start(&principal, "shared-request", &first_payload)
        .await
        .unwrap_or_else(|error| panic!("prepare first start: {error}"))
    else {
        panic!("first stopped session must require start");
    };
    let conflict = store
        .prepare_agent_start(
            &principal,
            "shared-request",
            &json!({"agent_id": SECOND_AGENT_ID}),
        )
        .await;
    assert!(matches!(conflict, Err(PersistenceError::CommandConflict)));
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(SECOND_AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read untouched second session: {error}"));
    let untouched: DurableAgentSession = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode untouched second session: {error}"));
    assert_eq!(untouched.public.runtime_status, "stopped");
    assert!(untouched.lifecycle_intent_id.is_empty());
    let reservation_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE request_id = 'shared-request' AND session_id = ? AND operation_id = ?",
    )
    .bind(AGENT_ID)
    .bind(&first.operation_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("inspect reservation: {error}"));
    assert_eq!(reservation_count, 1);
}

#[tokio::test]
async fn pending_lifecycle_request_blocks_non_lifecycle_command_admission() {
    let (store, principal, _directory) = fixture().await;
    let lifecycle_payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(_) = store
        .prepare_agent_start(&principal, "shared-command-request", &lifecycle_payload)
        .await
        .unwrap_or_else(|error| panic!("prepare reserved start: {error}"))
    else {
        panic!("stopped session must require start");
    };

    assert!(matches!(
        store
            .replay_command(
                &principal,
                "shared-command-request",
                "agent.create",
                &json!({"provider_id": "codex"}),
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(matches!(
        store
            .execute_message(
                &principal,
                "shared-command-request",
                "message.send",
                &json!({"content": "must not commit"}),
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    let message_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE room_id = 'general' AND event_json LIKE '%must not commit%'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("inspect rejected message: {error}"));
    assert_eq!(message_count, 0);
}

#[tokio::test]
async fn start_completion_derives_its_request_operation_binding() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "owned-request", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    assert!(matches!(
        store
            .complete_agent_start(
                &principal,
                "substituted-request",
                &payload,
                &start.operation_id,
                &started("owned-runtime", "owned-provider-thread"),
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "stale_start_confirmation",
            ..
        })
    ));
    authorize_start(
        &store,
        &principal,
        "owned-request",
        &payload,
        &start,
        "owned-runtime",
    )
    .await;
    store
        .complete_agent_start(
            &principal,
            "owned-request",
            &payload,
            &start.operation_id,
            &started("owned-runtime", "owned-provider-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete exact request: {error}"));
}

#[tokio::test]
async fn only_the_originating_operation_can_resume_or_replace_an_intent() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "owned-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare owned start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    for result in [
        store
            .prepare_agent_start(&principal, "different-start", &payload)
            .await
            .map(|_| ()),
        store
            .prepare_agent_stop(&principal, "opposite-stop", &payload)
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected {
                code: "operation_in_progress",
                ..
            })
        ));
    }
    authorize_start(
        &store,
        &principal,
        "owned-start",
        &payload,
        &start,
        "owned-runtime",
    )
    .await;
    store
        .complete_agent_start(
            &principal,
            "owned-start",
            &payload,
            &start.operation_id,
            &started("owned-runtime", "provider-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete owned start: {error}"));

    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "owned-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare owned stop: {error}"))
    else {
        panic!("running session must require stop");
    };
    for result in [
        store
            .prepare_agent_start(&principal, "opposite-start", &payload)
            .await
            .map(|_| ()),
        store
            .prepare_agent_stop(&principal, "different-stop", &payload)
            .await
            .map(|_| ()),
    ] {
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected {
                code: "operation_in_progress",
                ..
            })
        ));
    }
    store
        .authorize_agent_stop_effect(&principal, "owned-stop", &payload, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize owned stop: {error}"));
    store
        .record_agent_stop_effect("general", AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("record owned stop: {error}"));
    assert!(matches!(
        store
            .finalize_agent_stop(&principal, "different-stop", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "stale_stop_confirmation",
            ..
        })
    ));
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "owned-stop", &payload)
            .await
            .unwrap_or_else(|error| panic!("recover owned stop: {error}")),
        AgentStopPlan::Finalize
    ));
}

#[tokio::test]
async fn confirmed_stop_checkpoint_survives_restart_and_finalizes_without_an_effect() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-before-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    authorize_start(
        &store,
        &principal,
        "start-before-stop",
        &payload,
        &start,
        "runtime-before-restart",
    )
    .await;
    store
        .complete_agent_start(
            &principal,
            "start-before-stop",
            &payload,
            &start.operation_id,
            &started("runtime-before-restart", "provider-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "confirmed-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require stop");
    };
    store
        .authorize_agent_stop_effect(&principal, "confirmed-stop", &payload, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize confirmed stop: {error}"));
    store
        .record_agent_stop_effect("general", AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("record stop: {error}"));

    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile stop: {error}")),
        1
    );
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("repeat reconciliation: {error}")),
        0
    );
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = 'general' AND session_id = ?",
    )
    .bind(AGENT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read reconciled stop: {error}"));
    let durable = serde_json::from_str::<DurableAgentSession>(&encoded)
        .unwrap_or_else(|error| panic!("decode reconciled stop: {error}"));
    assert!(durable.lifecycle_intent_action.is_empty());
    assert!(durable.lifecycle_intent_status.is_empty());
    assert!(durable.runtime_handle_id.is_empty());
    assert!(durable.runtime_owner_id.is_empty());
    assert!(!durable.public.provider_session_active);
    let AgentStopPlan::Outcome(outcome) = store
        .prepare_agent_stop(&principal, "confirmed-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay confirmed stop: {error}"))
    else {
        panic!("startup recovery must persist the confirmed stop result");
    };
    assert_eq!(outcome.result["agent_session"]["runtime_status"], "stopped");
}

async fn authorize_start(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
    effect: &super::AgentStartEffect,
    runtime_handle_id: &str,
) {
    store
        .authorize_agent_start_effect(
            principal,
            request_id,
            payload,
            &effect.operation_id,
            "agent.start",
            runtime_handle_id,
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start effect: {error}"));
}
