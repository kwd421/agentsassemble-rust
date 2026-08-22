use agentsassemble_domain::DurableAgentSession;
use serde_json::json;

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan};
use crate::{
    PersistenceError,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};

fn started(handle: &str, provider_session_id: &str) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: handle.to_owned(),
        provider_session_id: provider_session_id.to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
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
    assert_eq!(durable.lifecycle_intent_action, "stop");
    assert_eq!(durable.lifecycle_intent_status, "effect_applied");
    assert!(durable.runtime_handle_id.is_empty());
    assert!(!durable.public.provider_session_active);
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "confirmed-stop", &payload)
            .await
            .unwrap_or_else(|error| panic!("recover confirmed stop: {error}")),
        AgentStopPlan::Finalize
    ));
    let outcome = store
        .finalize_agent_stop(&principal, "confirmed-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize confirmed stop: {error}"));
    assert_eq!(outcome.result["agent_session"]["runtime_status"], "stopped");
}

#[tokio::test]
async fn provider_session_reuse_requires_exact_durable_identity() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(first_start) = store
        .prepare_agent_start(&principal, "first-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare first start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    store
        .complete_agent_start(
            &principal,
            "first-start",
            &payload,
            &first_start.operation_id,
            &started("first-runtime", "durable-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete first start: {error}"));
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, "stop-between-starts", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require stop");
    };
    store
        .record_agent_stop_effect("general", AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("record stop: {error}"));
    store
        .finalize_agent_stop(&principal, "stop-between-starts", &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize stop: {error}"));
    let AgentStartPlan::Start(restart) = store
        .prepare_agent_start(&principal, "restart", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare restart: {error}"))
    else {
        panic!("stopped session must require restart");
    };
    let mismatch = store
        .complete_agent_start(
            &principal,
            "restart",
            &payload,
            &restart.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "second-runtime".to_owned(),
                provider_session_id: "substituted-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: true,
                provider_session_active: true,
            },
        )
        .await;
    assert!(matches!(
        mismatch,
        Err(PersistenceError::CommandRejected {
            code: "provider_session_mismatch",
            ..
        })
    ));
    store
        .complete_agent_start(
            &principal,
            "restart",
            &payload,
            &restart.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: "second-runtime".to_owned(),
                provider_session_id: "durable-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: true,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete exact reuse: {error}"));
}
