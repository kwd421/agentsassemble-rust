use agentsassemble_domain::{AuthenticatedPrincipal, ProviderCatalog};
use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartPlan, AgentTurnAssignment, LiveRuntimeReconciliation,
    ProviderTurnExecutionPhase, ProviderTurnReconciliationCandidate, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderCatalogService, ProviderRuntimeObservation, ProviderStartReservation,
};
use serde_json::json;

use super::reconcile_unowned_observation;
use crate::runtime_reconciliation::{
    RUNTIME_RECONCILIATION_TEST_LOCK, recover_exact_lifecycle_command,
    tests::{draft, local_principal},
};
use crate::{RoomRuntime, provider_recovery_tracker::ProviderRecoveryTracker};

#[tokio::test]
async fn production_replay_helper_observes_gone_before_reenabling_start() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("16193216-8799-4f67-ad17-f05c7da0f433", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "67e86a68-c52b-4ffc-8039-c908a33a9150",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    let principal = local_principal();
    let created = store
        .execute_agent_create(
            &principal,
            "create-recovery-agent",
            &json!({"provider_id": "codex"}),
            &draft(
                directory.path(),
                "codex-00000000-0000-5000-8000-000000000201",
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("create agent: {error}"));
    let session_id = created.result["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"));
    let payload = json!({"agent_id": session_id});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "live-helper-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    let provider_adapter = ProviderAdapter::new();
    let reservation = provider_adapter
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve provider start: {error}"));
    store
        .authorize_agent_start_effect(
            &principal,
            "live-helper-start",
            &payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            &reservation.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize provider start: {error}"));
    store
        .mark_agent_start_unconfirmed(
            &principal,
            session_id,
            &effect.operation_id,
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            "runtime_start_unconfirmed",
            "provider effect boundary was uncertain",
        )
        .await
        .unwrap_or_else(|error| panic!("mark start unconfirmed: {error}"));
    assert_eq!(
        recover_exact_lifecycle_command(
            &store,
            &provider_adapter,
            &principal,
            "live-helper-start",
            "agent.start",
            &payload,
        )
        .await
        .unwrap_or_else(|error| panic!("recover exact start: {error}")),
        LiveRuntimeReconciliation::RetryOriginalEffect
    );
    let AgentStartPlan::Start(retry) = store
        .prepare_agent_start(&principal, "live-helper-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("re-enter start: {error}"))
    else {
        panic!("gone pre-effect generation must reopen the original start");
    };
    let next = provider_adapter
        .reserve_start(&retry.session)
        .await
        .unwrap_or_else(|error| panic!("reserve post-recovery generation: {error}"));
    assert_ne!(next.runtime_lease_token, reservation.runtime_lease_token);
    provider_adapter
        .cancel_start_reservation("general", session_id, &next)
        .await;
}

async fn create_live_scan_agent() -> (
    tempfile::TempDir,
    SqliteStore,
    AuthenticatedPrincipal,
    String,
) {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("16193216-8799-4f67-ad17-f05c7da0f434", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "67e86a68-c52b-4ffc-8039-c908a33a9151",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    let principal = local_principal();
    let created = store
        .execute_agent_create(
            &principal,
            "create-live-provider-agent",
            &json!({"provider_id": "opencode"}),
            &draft(
                directory.path(),
                "codex-00000000-0000-5000-8000-000000000211",
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("create agent: {error}"));
    let session_id = created.result["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"))
        .to_owned();
    (directory, store, principal, session_id)
}

struct LiveScanFixture {
    _directory: tempfile::TempDir,
    store: SqliteStore,
    session_id: String,
    turn_generation: u64,
    assignment: AgentTurnAssignment,
    candidate: ProviderTurnReconciliationCandidate,
    lease_owner: ProviderAdapter,
    reservation: ProviderStartReservation,
    fresh_adapter: ProviderAdapter,
    rooms: RoomRuntime,
}

async fn stage_live_scan_fixture() -> LiveScanFixture {
    let (directory, store, principal, session_id) = create_live_scan_agent().await;
    let payload = json!({"agent_id": session_id});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "stage-live-provider", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare start");
    };
    let lease_owner = ProviderAdapter::new();
    let reservation = lease_owner
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve exact runtime lease: {error}"));
    store
        .authorize_agent_start_effect(
            &principal,
            "stage-live-provider",
            &payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            &reservation.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize staged runtime: {error}"));
    store
        .complete_agent_start(
            &principal,
            "stage-live-provider",
            &payload,
            &effect.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: reservation.runtime_handle_id.clone(),
                runtime_owner_id: reservation.runtime_owner_id.clone(),
                runtime_lease_token: reservation.runtime_lease_token.clone(),
                provider_session_id: "provider-session-live-scan".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("persist staged runtime: {error}"));
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "live-provider-message",
            "message.send",
            &json!({"content": "@Terra retain this exact turn"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign live provider turn: {error}"));
    let assignment = mutation
        .assignments
        .first()
        .unwrap_or_else(|| panic!("message did not assign provider turn"));
    store
        .authorize_provider_turn_start(
            "general",
            &session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize provider turn: {error}"));
    let mut page = store
        .load_provider_turn_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("load live provider candidate: {error}"));
    let candidate = page
        .candidates
        .pop()
        .unwrap_or_else(|| panic!("missing live provider candidate"));
    let turn_generation = assignment.turn_generation;
    let fresh_adapter = ProviderAdapter::new();
    let rooms = RoomRuntime::with_provider_adapter(
        store.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
        fresh_adapter.clone(),
    );

    LiveScanFixture {
        _directory: directory,
        store,
        session_id,
        turn_generation,
        assignment: assignment.clone(),
        candidate,
        lease_owner,
        reservation,
        fresh_adapter,
        rooms,
    }
}

#[tokio::test]
async fn exact_queued_recovery_claim_blocks_duplicate_handoff_until_owner_release() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let fixture = stage_live_scan_fixture().await;
    let tracker = ProviderRecoveryTracker::default();
    let owner = tracker
        .try_claim(&fixture.assignment)
        .unwrap_or_else(|| panic!("first exact recovery owner was not admitted"));
    assert!(tracker.try_claim(&fixture.assignment).is_none());
    drop(owner);
    assert!(tracker.try_claim(&fixture.assignment).is_some());
    fixture
        .lease_owner
        .cancel_start_reservation("general", &fixture.session_id, &fixture.reservation)
        .await;
    fixture
        .rooms
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown recovery-claim room: {error}"));
}

#[tokio::test]
async fn live_provider_scan_revisits_a_transient_lease_until_exact_gone() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let fixture = stage_live_scan_fixture().await;

    reconcile_unowned_observation(
        &fixture.store,
        &fixture.fresh_adapter,
        &fixture.rooms,
        &fixture.candidate,
        Some(ProviderRuntimeObservation::LeaseUncertain {
            handle_id: fixture.reservation.runtime_handle_id.clone(),
            owner_id: fixture.reservation.runtime_owner_id.clone(),
            reason_code: "runtime_lease_active".to_owned(),
        }),
    )
    .await
    .unwrap_or_else(|error| panic!("retain transient lease candidate: {error}"));
    assert_eq!(
        fixture
            .store
            .provider_turn_execution("general", &fixture.session_id, fixture.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read lease-uncertain execution: {error}"))
            .phase,
        ProviderTurnExecutionPhase::StartDispatching
    );

    fixture
        .lease_owner
        .cancel_start_reservation("general", &fixture.session_id, &fixture.reservation)
        .await;
    reconcile_unowned_observation(
        &fixture.store,
        &fixture.fresh_adapter,
        &fixture.rooms,
        &fixture.candidate,
        Some(ProviderRuntimeObservation::Gone),
    )
    .await
    .unwrap_or_else(|error| panic!("finalize later exact Gone: {error}"));
    assert_eq!(
        fixture
            .store
            .provider_turn_execution("general", &fixture.session_id, fixture.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read Gone-finalized execution: {error}"))
            .phase,
        ProviderTurnExecutionPhase::Failed
    );
    fixture
        .rooms
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown room runtime: {error}"));
}

#[tokio::test]
async fn shutdown_terminalizes_a_blocking_turn_before_releasing_its_runtime_lease() {
    let _serial = RUNTIME_RECONCILIATION_TEST_LOCK.lock().await;
    let fixture = stage_live_scan_fixture().await;
    let shutdown_rooms = RoomRuntime::with_provider_adapter(
        fixture.store.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
        fixture.lease_owner.clone(),
    );

    shutdown_rooms
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("checkpoint blocking turn during shutdown: {error}"));
    assert_eq!(
        fixture
            .store
            .provider_turn_execution("general", &fixture.session_id, fixture.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read shutdown-finalized execution: {error}"))
            .phase,
        ProviderTurnExecutionPhase::Failed
    );
    assert!(
        fixture
            .store
            .load_active_provider_turn_reconciliation_candidate("general", &fixture.session_id)
            .await
            .unwrap_or_else(|error| panic!("reload blocking shutdown candidate: {error}"))
            .is_none()
    );
    fixture
        .rooms
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown scan room runtime: {error}"));
}
