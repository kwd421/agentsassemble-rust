use std::path::Path;

use agentsassemble_domain::{AuthenticatedPrincipal, ProviderCatalog};
use agentsassemble_persistence::{AgentStartPlan, PersistenceError, SqliteStore};
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::{reconcile_dynamic_candidates, tests::draft, tests::local_principal};
use crate::RoomRuntime;

#[tokio::test]
async fn lost_command_owner_recovers_effect_inflight_without_browser_request_identity() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("create fixture: {error}"));
    let (store, principal, session_id, payload) = owner_loss_fixture(directory.path()).await;
    let provider_adapter = ProviderAdapter::new();
    let rooms = RoomRuntime::with_provider_adapter(
        store.clone(),
        ProviderCatalogService::fixed(ProviderCatalog::default()),
        provider_adapter.clone(),
    );
    let command_owner = rooms.claim_lifecycle_command(
        "general",
        &principal.principal_id,
        "owner-loss-start",
        "agent.start",
    );
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "owner-loss-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must prepare a start effect");
    };
    let reservation = provider_adapter
        .reserve_start(&effect.session)
        .await
        .unwrap_or_else(|error| panic!("reserve start: {error}"));
    store
        .authorize_agent_start_effect(
            &principal,
            "owner-loss-start",
            &payload,
            &effect.operation_id,
            "agent.start",
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize start: {error}"));
    let candidate = store
        .load_runtime_reconciliation_page(None)
        .await
        .unwrap_or_else(|error| panic!("load candidate: {error}"))
        .candidates
        .pop()
        .unwrap_or_else(|| panic!("effect-inflight start had no candidate"));
    let cancellation = CancellationToken::new();
    reconcile_dynamic_candidates(
        &store,
        &provider_adapter,
        &rooms,
        vec![candidate.clone()],
        &cancellation,
    )
    .await;
    assert!(
        store
            .load_runtime_reconciliation_candidate("general", &session_id)
            .await
            .unwrap_or_else(|error| panic!("reload active candidate: {error}"))
            .is_some()
    );

    drop(command_owner);
    reconcile_dynamic_candidates(
        &store,
        &provider_adapter,
        &rooms,
        vec![candidate],
        &cancellation,
    )
    .await;
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "owner-loss-start", &payload)
            .await,
        Err(PersistenceError::StoredCommandRejected { code, .. })
            if code == "runtime_start_recovered_gone"
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "owner-loss-replacement", &payload)
            .await,
        Ok(AgentStartPlan::Start(_))
    ));
    rooms
        .shutdown()
        .await
        .unwrap_or_else(|error| panic!("shutdown room runtime: {error}"));
}

async fn owner_loss_fixture(root: &Path) -> (SqliteStore, AuthenticatedPrincipal, String, Value) {
    let store = SqliteStore::open_path(&root.join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("36193216-8799-4f67-ad17-f05c7da0f433", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
    store
        .create_room_for_local_operator(
            "87e86a68-c52b-4ffc-8039-c908a33a9150",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    let principal = local_principal();
    let created = store
        .execute_agent_create(
            &principal,
            "create-owner-loss-agent",
            &json!({"provider_id": "opencode"}),
            &draft(root, "codex-00000000-0000-5000-8000-000000000203"),
        )
        .await
        .unwrap_or_else(|error| panic!("create agent: {error}"));
    let session_id = created.result["agent_session"]["session_id"]
        .as_str()
        .unwrap_or_else(|| panic!("created session has no id"))
        .to_owned();
    let payload = json!({"agent_id": session_id});
    (store, principal, session_id, payload)
}
