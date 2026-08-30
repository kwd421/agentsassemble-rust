use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartPlan, AgentStopPlan, LiveRuntimeReconciliation, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderStartReservation};
use serde_json::{Value, json};

use super::{
    RUNTIME_RECONCILIATION_TEST_LOCK, reconcile_runtime_ownership, recover_exact_lifecycle_command,
};
use crate::runtime_reconciliation::tests::{draft, dynamic_recovery_store, local_principal};

struct ConfirmedAbsenceFixture {
    _directory: tempfile::TempDir,
    store: SqliteStore,
    provider_adapter: ProviderAdapter,
    principal: agentsassemble_domain::AuthenticatedPrincipal,
    session_id: String,
    payload: Value,
    reservation: ProviderStartReservation,
}

#[tokio::test]
async fn exact_live_stop_releases_its_captured_tombstone_after_commit() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let fixture = confirmed_absence_fixture("000000000205").await;
    let AgentStopPlan::Stop(effect) = fixture
        .store
        .prepare_agent_stop(&fixture.principal, "lost-stop-checkpoint", &fixture.payload)
        .await
        .unwrap_or_else(|error| panic!("prepare exact stop: {error}"))
    else {
        panic!("durable running session must prepare a stop effect");
    };
    fixture
        .store
        .authorize_agent_stop_effect(
            &fixture.principal,
            "lost-stop-checkpoint",
            &fixture.payload,
            &effect.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize exact stop: {error}"));

    assert_eq!(
        recover_exact_lifecycle_command(
            &fixture.store,
            &fixture.provider_adapter,
            &fixture.principal,
            "lost-stop-checkpoint",
            "agent.stop",
            &fixture.payload,
        )
        .await
        .unwrap_or_else(|error| panic!("recover exact stop: {error}")),
        LiveRuntimeReconciliation::RetryOriginalEffect
    );
    assert!(matches!(
        fixture
            .store
            .prepare_agent_stop(&fixture.principal, "lost-stop-checkpoint", &fixture.payload,)
            .await
            .unwrap_or_else(|error| panic!("reload exact stop: {error}")),
        AgentStopPlan::Finalize
    ));
    fixture
        .store
        .finalize_agent_stop(&fixture.principal, "lost-stop-checkpoint", &fixture.payload)
        .await
        .unwrap_or_else(|error| panic!("finalize exact stop: {error}"));
    let AgentStartPlan::Start(retry) = fixture
        .store
        .prepare_agent_start(&fixture.principal, "fresh-after-stop", &fixture.payload)
        .await
        .unwrap_or_else(|error| panic!("prepare fresh start: {error}"))
    else {
        panic!("finalized stop must permit a fresh start");
    };
    let next = fixture
        .provider_adapter
        .reserve_start(&retry.session)
        .await
        .unwrap_or_else(|error| panic!("reserve fresh start: {error}"));
    assert_ne!(
        next.runtime_lease_token,
        fixture.reservation.runtime_lease_token
    );
    fixture
        .provider_adapter
        .cancel_start_reservation("general", &fixture.session_id, &next)
        .await;
}

#[tokio::test]
async fn startup_gone_accepts_an_active_runtime_without_a_lifecycle_action() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let fixture = confirmed_absence_fixture("000000000206").await;
    drop(fixture.provider_adapter);
    let fresh_adapter = ProviderAdapter::new();
    let reconciled = reconcile_runtime_ownership(&fixture.store, &fresh_adapter)
        .await
        .unwrap_or_else(|error| panic!("reconcile cold runtime absence: {error}"));
    assert_eq!(reconciled.reconciled_sessions, 1);
    assert!(reconciled.assignments.is_empty());
    assert!(
        fixture
            .store
            .load_runtime_reconciliation_candidate("general", &fixture.session_id)
            .await
            .unwrap_or_else(|error| panic!("reload cold reconciliation: {error}"))
            .is_none()
    );
    let AgentStartPlan::Start(retry) = fixture
        .store
        .prepare_agent_start(
            &fixture.principal,
            "fresh-after-cold-gone",
            &fixture.payload,
        )
        .await
        .unwrap_or_else(|error| panic!("prepare post-restart start: {error}"))
    else {
        panic!("cold Gone must permit a fresh start");
    };
    let next = fresh_adapter
        .reserve_start(&retry.session)
        .await
        .unwrap_or_else(|error| panic!("reserve post-restart start: {error}"));
    fresh_adapter
        .cancel_start_reservation("general", &fixture.session_id, &next)
        .await;
}

async fn confirmed_absence_fixture(id_suffix: &str) -> ConfirmedAbsenceFixture {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create absence fixture: {error}"));
    let store = dynamic_recovery_store(directory.path()).await;
    let principal = local_principal();
    let failed_draft = draft(
        directory.path(),
        &format!("codex-00000000-0000-5000-8000-{id_suffix}"),
    );
    let created = store
        .execute_agent_create(
            &principal,
            &format!("create-confirmed-absence-{id_suffix}"),
            &json!({"provider_id": "opencode"}),
            &failed_draft,
        )
        .await
        .unwrap_or_else(|error| panic!("create confirmed absence agent: {error}"));
    let session_id = created.result["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created confirmed absence session has no id"))
        .to_owned();
    let payload = json!({"agent_id": session_id});
    let request_id = format!("stage-confirmed-absence-{id_suffix}");
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, &request_id, &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare confirmed absence: {error}"))
    else {
        panic!("stopped session must prepare confirmed absence");
    };
    let provider_adapter = ProviderAdapter::new();
    let reservation = provider_adapter
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve confirmed absence: {error}"));
    let authorized = store
        .authorize_agent_start_effect(
            &principal,
            &request_id,
            &payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            &reservation.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize confirmed absence: {error}"));
    let Err(failure) = provider_adapter.start_reserved(&authorized.session).await else {
        panic!("non-provider fixture executable must create confirmed absence");
    };
    assert!(failure.runtime_stopped);
    store
        .complete_agent_start(
            &principal,
            &request_id,
            &payload,
            &effect.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: reservation.runtime_handle_id.clone(),
                runtime_owner_id: reservation.runtime_owner_id.clone(),
                runtime_lease_token: reservation.runtime_lease_token.clone(),
                provider_session_id: format!("provider-session-{id_suffix}"),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("stage durable running authority: {error}"));
    ConfirmedAbsenceFixture {
        _directory: directory,
        store,
        provider_adapter,
        principal,
        session_id,
        payload,
        reservation,
    }
}
