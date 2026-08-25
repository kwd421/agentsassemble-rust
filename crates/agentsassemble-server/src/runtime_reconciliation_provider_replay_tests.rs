use agentsassemble_persistence::{AgentStartPlan, LiveRuntimeReconciliation, SqliteStore};
use agentsassemble_provider::ProviderAdapter;
use serde_json::json;

use super::tests::{draft, local_principal};
use super::{RUNTIME_RECONCILIATION_TEST_LOCK, recover_exact_lifecycle_command};

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
