use serde_json::{Value, json};

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan};
use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};

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
    authorize_start(
        &store,
        &principal,
        "first-start",
        &payload,
        &first_start,
        "first-runtime",
    )
    .await;
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
        .authorize_agent_stop_effect(
            &principal,
            "stop-between-starts",
            &payload,
            &stop.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize stop between starts: {error}"));
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
    authorize_start(
        &store,
        &principal,
        "restart",
        &payload,
        &restart,
        "second-runtime",
    )
    .await;
    let mismatch = store
        .complete_agent_start(
            &principal,
            "restart",
            &payload,
            &restart.operation_id,
            &reused_started("substituted-thread"),
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
            &reused_started("durable-thread"),
        )
        .await
        .unwrap_or_else(|error| panic!("complete exact reuse: {error}"));
}

#[tokio::test]
async fn unconfirmed_start_rejects_a_substituted_runtime_identity() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "identity-bound-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare identity-bound start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    authorize_start(
        &store,
        &principal,
        "identity-bound-start",
        &payload,
        &start,
        "authorized-runtime",
    )
    .await;
    assert!(matches!(
        store
            .mark_agent_start_unconfirmed(
                &principal,
                AGENT_ID,
                &start.operation_id,
                "substituted-runtime",
                "supervisor-instance-1",
                "runtime_start_unconfirmed",
                "Provider initialization was not confirmed.",
            )
            .await,
        Err(PersistenceError::CommandRejected {
            code: "runtime_owner_mismatch",
            ..
        })
    ));
    let current = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("reload identity-bound start: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("identity-bound start lost its candidate"));
    assert_eq!(current.session.runtime_handle_id, "authorized-runtime");
    assert_eq!(current.session.lifecycle_intent_status, "effect_inflight");
}

fn started(handle: &str, provider_session_id: &str) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: handle.to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        provider_session_id: provider_session_id.to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
}

fn reused_started(provider_session_id: &str) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: "second-runtime".to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        provider_session_id: provider_session_id.to_owned(),
        runtime_reused: false,
        provider_session_reused: true,
        provider_session_active: true,
    }
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
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start effect: {error}"));
}
