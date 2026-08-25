use serde_json::json;

use super::{
    AgentStartPlan,
    tests::{AGENT_ID, fixture},
};
use crate::{LiveRuntimeReconciliation, PersistenceError, RuntimeReconciliationObservation};

#[tokio::test]
async fn exact_live_replay_reenters_start_only_after_gone_proof() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "live-gone-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    authorize_start(&store, &principal, "live-gone-start", &payload, &effect).await;
    store
        .mark_agent_start_unconfirmed(
            &principal,
            AGENT_ID,
            &effect.operation_id,
            "runtime-live-gone",
            "supervisor-live",
            "runtime_start_unconfirmed",
            "launch outcome was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    let candidate = store
        .load_lifecycle_reconciliation_candidate(
            &principal,
            "live-gone-start",
            "agent.start",
            &payload,
        )
        .await
        .unwrap_or_else(|error| panic!("load exact recovery candidate: {error}"));
    assert_eq!(
        store
            .apply_live_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone,)
            .await
            .unwrap_or_else(|error| panic!("apply gone proof: {error}")),
        LiveRuntimeReconciliation::RetryOriginalEffect
    );
    let AgentStartPlan::Start(retry) = store
        .prepare_agent_start(&principal, "live-gone-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("re-enter exact start: {error}"))
    else {
        panic!("gone proof must re-enter only the original effect");
    };
    assert!(retry.session.runtime_handle_id.is_empty());
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-start", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "operation_in_progress",
            ..
        })
    ));
}

#[tokio::test]
async fn exact_effect_inflight_replay_remains_live_recovery_authority() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "live-effect-inflight", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare effect-inflight start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    authorize_start(
        &store,
        &principal,
        "live-effect-inflight",
        &payload,
        &effect,
    )
    .await;
    let candidate = store
        .load_lifecycle_reconciliation_candidate(
            &principal,
            "live-effect-inflight",
            "agent.start",
            &payload,
        )
        .await
        .unwrap_or_else(|error| panic!("load effect-inflight candidate: {error}"));
    assert_eq!(candidate.session.lifecycle_intent_status, "effect_inflight");
    assert_eq!(
        store
            .apply_live_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
            .await
            .unwrap_or_else(|error| panic!("apply effect-inflight gone proof: {error}")),
        LiveRuntimeReconciliation::RetryOriginalEffect
    );
}

#[tokio::test]
async fn exact_live_replay_resumes_only_the_adopted_owned_runtime() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "live-adopted-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    authorize_start(&store, &principal, "live-adopted-start", &payload, &effect).await;
    store
        .mark_agent_start_unconfirmed(
            &principal,
            AGENT_ID,
            &effect.operation_id,
            "runtime-live-adopted",
            "supervisor-live",
            "provider_session_unconfirmed",
            "attachment outcome was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    let candidate = store
        .load_lifecycle_reconciliation_candidate(
            &principal,
            "live-adopted-start",
            "agent.start",
            &payload,
        )
        .await
        .unwrap_or_else(|error| panic!("load exact recovery candidate: {error}"));
    let profile_key = candidate.session.runtime_profile_key.clone();
    assert_eq!(
        store
            .apply_live_runtime_reconciliation(
                &candidate,
                &RuntimeReconciliationObservation::Adopted {
                    handle_id: "runtime-live-adopted".to_owned(),
                    previous_owner_id: "supervisor-live".to_owned(),
                    new_owner_id: "supervisor-live".to_owned(),
                    runtime_profile_key: profile_key,
                },
            )
            .await
            .unwrap_or_else(|error| panic!("apply adoption proof: {error}")),
        LiveRuntimeReconciliation::RetryOriginalEffect
    );
    let AgentStartPlan::Start(retry) = store
        .prepare_agent_start(&principal, "live-adopted-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("resume exact start: {error}"))
    else {
        panic!("adoption proof must resume only the original effect");
    };
    assert_eq!(retry.session.runtime_handle_id, "runtime-live-adopted");
    assert_eq!(retry.session.runtime_owner_id, "supervisor-live");
}

#[tokio::test]
async fn startup_gone_terminalizes_old_start_and_unblocks_a_new_request() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "abandoned-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    authorize_start(&store, &principal, "abandoned-start", &payload, &effect).await;
    store
        .mark_agent_start_unconfirmed(
            &principal,
            AGENT_ID,
            &effect.operation_id,
            "runtime-abandoned",
            "supervisor-dead",
            "runtime_start_unconfirmed",
            "launch outcome was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    let candidate = store
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load startup candidate: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("unconfirmed start must be a startup candidate"));
    store
        .apply_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("terminalize abandoned start: {error}"));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "abandoned-start", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected {
            code,
            ..
        }) if code == "runtime_start_recovered_gone"
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "replacement-after-recovery", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare replacement start: {error}")),
        AgentStartPlan::Start(_)
    ));
}

#[tokio::test]
async fn previous_supervisor_request_cannot_use_live_effect_reentry_after_reopen() {
    let (store, principal, directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "previous-supervisor-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    authorize_start(
        &store,
        &principal,
        "previous-supervisor-start",
        &payload,
        &effect,
    )
    .await;
    store
        .mark_agent_start_unconfirmed(
            &principal,
            AGENT_ID,
            &effect.operation_id,
            "runtime-previous-supervisor",
            "supervisor-previous",
            "runtime_start_unconfirmed",
            "launch outcome was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    drop(store);

    let reopened = super::SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("reopen store: {error}"));
    assert!(matches!(
        reopened
            .load_lifecycle_reconciliation_candidate(
                &principal,
                "previous-supervisor-start",
                "agent.start",
                &payload,
            )
            .await,
        Err(PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            ..
        })
    ));
    let candidate = reopened
        .load_runtime_reconciliation_candidates()
        .await
        .unwrap_or_else(|error| panic!("load server-owned candidate: {error}"))
        .pop()
        .unwrap_or_else(|| panic!("old request must remain server-recoverable"));
    reopened
        .apply_runtime_reconciliation(&candidate, &RuntimeReconciliationObservation::Gone)
        .await
        .unwrap_or_else(|error| panic!("terminalize old request: {error}"));
    assert!(matches!(
        reopened
            .prepare_agent_start(&principal, "previous-supervisor-start", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected { code, .. })
            if code == "runtime_start_recovered_gone"
    ));
}

async fn authorize_start(
    store: &super::SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    payload: &serde_json::Value,
    effect: &super::AgentStartEffect,
) {
    let (handle_id, owner_id) = match request_id {
        "live-gone-start" => ("runtime-live-gone", "supervisor-live"),
        "live-effect-inflight" => ("runtime-live-effect-inflight", "supervisor-live"),
        "live-adopted-start" => ("runtime-live-adopted", "supervisor-live"),
        "abandoned-start" => ("runtime-abandoned", "supervisor-dead"),
        "previous-supervisor-start" => ("runtime-previous-supervisor", "supervisor-previous"),
        _ => panic!("unexpected test request"),
    };
    store
        .authorize_agent_start_effect(
            principal,
            request_id,
            payload,
            &effect.operation_id,
            "agent.start",
            handle_id,
            owner_id,
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start effect: {error}"));
}
