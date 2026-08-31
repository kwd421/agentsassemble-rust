use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartPlan, PersistenceError, ProviderTurnEffectPhase,
    ProviderTurnInterruptEffect, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderStartReservation};
use serde_json::json;

use super::apply_exact_interrupt;
use crate::runtime_reconciliation::tests::{draft, local_principal};

struct PreSlotFixture {
    _directory: tempfile::TempDir,
    store: SqliteStore,
    session_id: String,
    effect: ProviderTurnInterruptEffect,
    lease_owner: ProviderAdapter,
    reservation: ProviderStartReservation,
}

#[tokio::test]
async fn pre_slot_interrupt_hands_claim_to_recovery_without_ttl_wait() {
    let fixture = stage_pre_slot_interrupt().await;
    assert!(matches!(
        Box::pin(apply_exact_interrupt(
            &fixture.store,
            &ProviderAdapter::new(),
            &fixture.effect,
        ))
        .await,
        Err(PersistenceError::CommandUnresolved { .. })
    ));
    let handed_off = fixture
        .store
        .provider_turn_interrupt_effect(
            "general",
            &fixture.session_id,
            fixture.effect.turn_generation,
        )
        .await
        .unwrap_or_else(|error| panic!("load handed-off interrupt: {error}"));
    assert_eq!(handed_off.phase, ProviderTurnEffectPhase::RecoveryRequired);
    let recovery = fixture
        .store
        .claim_provider_turn_interrupt_recovery(&handed_off, "10000000-0000-4000-8000-000000000401")
        .await
        .unwrap_or_else(|error| panic!("claim handed-off interrupt immediately: {error}"));
    fixture
        .store
        .release_provider_interrupt_recovery_claim(&recovery)
        .await
        .unwrap_or_else(|error| panic!("release pre-I/O recovery attempt: {error}"));
    fixture
        .store
        .claim_provider_turn_interrupt_recovery(&handed_off, "10000000-0000-4000-8000-000000000402")
        .await
        .unwrap_or_else(|error| panic!("reclaim released recovery interrupt: {error}"));
    fixture
        .lease_owner
        .cancel_start_reservation("general", &fixture.session_id, &fixture.reservation)
        .await;
}

async fn stage_pre_slot_interrupt() -> PreSlotFixture {
    let directory =
        tempfile::tempdir().unwrap_or_else(|error| panic!("create interrupt fixture: {error}"));
    let store = bootstrap_store(directory.path()).await;
    let principal = local_principal();
    let created = store
        .execute_agent_create(
            &principal,
            "create-pre-slot-interrupt-agent",
            &json!({"provider_id": "opencode"}),
            &draft(
                directory.path(),
                "codex-00000000-0000-5000-8000-000000000401",
            ),
        )
        .await
        .unwrap_or_else(|error| panic!("create pre-slot interrupt agent: {error}"));
    let session_id = created.result["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created pre-slot session has no id"))
        .to_owned();
    let payload = json!({"agent_id": session_id});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "start-pre-slot-interrupt-agent", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare pre-slot runtime: {error}"))
    else {
        panic!("stopped session must prepare a runtime start");
    };
    let lease_owner = ProviderAdapter::new();
    let reservation = lease_owner
        .reserve_start(&start.session)
        .await
        .unwrap_or_else(|error| panic!("reserve pre-slot runtime: {error}"));
    store
        .authorize_agent_start_effect(
            &principal,
            "start-pre-slot-interrupt-agent",
            &payload,
            &start.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            &reservation.runtime_lease_token,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize pre-slot runtime: {error}"));
    store
        .complete_agent_start(
            &principal,
            "start-pre-slot-interrupt-agent",
            &payload,
            &start.operation_id,
            &AgentRuntimeStarted {
                runtime_handle_id: reservation.runtime_handle_id.clone(),
                runtime_owner_id: reservation.runtime_owner_id.clone(),
                runtime_lease_token: reservation.runtime_lease_token.clone(),
                provider_session_id: "provider-session-pre-slot".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await
        .unwrap_or_else(|error| panic!("persist pre-slot runtime: {error}"));
    let mutation = store
        .execute_message_with_turn(
            &principal,
            "pre-slot-source",
            "message.send",
            &json!({"content": "@Agent interrupt before its slot is installed"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign pre-slot turn: {error}"));
    assert_eq!(mutation.assignments.len(), 1);
    let accepted = store
        .execute_agent_interrupt(&principal, "pre-slot-interrupt", &payload)
        .await
        .unwrap_or_else(|error| panic!("accept pre-slot interrupt: {error}"));
    PreSlotFixture {
        _directory: directory,
        store,
        session_id,
        effect: accepted
            .interrupt_effect
            .unwrap_or_else(|| panic!("fresh pre-slot interrupt has no effect")),
        lease_owner,
        reservation,
    }
}

async fn bootstrap_store(root: &std::path::Path) -> SqliteStore {
    let store = SqliteStore::open_path(&root.join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open interrupt store: {error}"));
    store
        .bootstrap_local_authority("36193216-8799-4f67-ad17-f05c7da0f433", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap interrupt identity: {error}"));
    store
        .create_room_for_local_operator(
            "87e86a68-c52b-4ffc-8039-c908a33a9150",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create interrupt room: {error}"));
    store
}
