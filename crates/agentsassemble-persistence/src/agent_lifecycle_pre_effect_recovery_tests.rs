use serde_json::{Value, json};

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan};
use crate::{
    PersistenceError, SqliteStore,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};

#[tokio::test]
async fn abandoned_pre_effect_start_is_terminalized_without_spawning() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(_) = store
        .prepare_agent_start(&principal, "unobserved-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare unobserved start: {error}"))
    else {
        panic!("stopped session must require an effect");
    };
    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile unobserved start: {error}")),
        1
    );
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "unobserved-start", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected { code, .. })
            if code == "runtime_start_abandoned_before_effect"
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-after-unobserved", &payload)
            .await,
        Ok(AgentStartPlan::Start(_))
    ));
}

#[tokio::test]
async fn restart_rejects_a_pre_effect_stop_without_claiming_runtime_shutdown() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-before-prepared-stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require start");
    };
    authorize_start(&store, &principal, &payload, &start).await;
    store
        .complete_agent_start(
            &principal,
            "start-before-prepared-stop",
            &payload,
            &start.operation_id,
            &started(),
        )
        .await
        .unwrap_or_else(|error| panic!("complete start: {error}"));
    let AgentStopPlan::Stop(_) = store
        .prepare_agent_stop(&principal, "prepared-stop-owner-lost", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require stop");
    };

    assert_eq!(
        store
            .reconcile_agent_sessions_after_restart()
            .await
            .unwrap_or_else(|error| panic!("reconcile prepared stop: {error}")),
        1
    );
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "prepared-stop-owner-lost", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected { code, .. })
            if code == "runtime_stop_abandoned_before_effect"
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "new-start-after-owner-loss", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare new start: {error}")),
        AgentStartPlan::Start(_)
    ));
}

async fn authorize_start(
    store: &SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    payload: &Value,
    effect: &super::AgentStartEffect,
) {
    store
        .authorize_agent_start_effect(
            principal,
            "start-before-prepared-stop",
            payload,
            &effect.operation_id,
            "agent.start",
            "runtime-before-prepared-stop",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start effect: {error}"));
}

fn started() -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: "runtime-before-prepared-stop".to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        runtime_lease_token: "lease-generation-1".to_owned(),
        provider_session_id: "provider-thread".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    }
}
