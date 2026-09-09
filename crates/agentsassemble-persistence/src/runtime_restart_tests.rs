use agentsassemble_domain::AuthenticatedPrincipal;
use serde_json::json;

use super::tests::{AGENT_ID, fixture};
use crate::{
    AgentRuntimeStarted, AgentStartPlan, PersistenceError, RoomMutationAuthority::TrustedPrincipal,
    RuntimeRestartPhase, SqliteStore,
};

const OPERATION: &str = "2c368489-1783-454f-b40e-947504f51014";
const NEXT_OPERATION: &str = "2c368489-1783-454f-b40e-947504f51015";

#[tokio::test]
async fn restart_admission_precedes_launch_and_abort_releases_retry()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let record = store.prepare_runtime_restart(OPERATION).await?;
    assert_eq!(record.phase, RuntimeRestartPhase::Quiescing);
    assert!(record.targets.is_empty());
    assert_eq!(store.prepare_runtime_restart(OPERATION).await?, record);
    assert!(
        matches!(store.prepare_runtime_restart(NEXT_OPERATION).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "runtime_restart_busy")
    );
    let payload = json!({"agent_id": AGENT_ID});
    assert!(
        matches!(store.prepare_agent_start(TrustedPrincipal(&principal), "after-restart", &payload).await,
        Err(PersistenceError::CommandUnresolved { code, .. }) if code == "runtime_restarting")
    );
    assert!(
        matches!(store.execute_message_with_turn(&principal, "during-restart", "message.send", &json!({"content":"hello"})).await,
        Err(PersistenceError::CommandUnresolved { code, .. }) if code == "runtime_restarting")
    );
    let aborted = store.abort_runtime_restart(OPERATION).await?;
    assert_eq!(aborted.phase, RuntimeRestartPhase::Aborted);
    assert_eq!(store.abort_runtime_restart(OPERATION).await?, aborted);
    assert_eq!(store.prepare_runtime_restart(OPERATION).await?, aborted);
    assert!(matches!(
        store
            .prepare_agent_start(TrustedPrincipal(&principal), "after-restart", &payload)
            .await?,
        AgentStartPlan::Start(_)
    ));
    assert!(
        matches!(store.prepare_runtime_restart(NEXT_OPERATION).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "runtime_restart_busy")
    );
    Ok(())
}

#[tokio::test]
async fn restart_retains_targets_and_replays_but_refuses_assigned_turns()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    start_local_fixture(&store, &principal).await?;
    let queued_event = queue_local_fixture(&store, &principal).await?;
    let record = store.prepare_runtime_restart(OPERATION).await?;
    assert_eq!(record.targets.len(), 1);
    assert_eq!(record.targets[0].room_id, "general");
    assert_eq!(record.targets[0].session_id, AGENT_ID);
    assert!(!record.targets[0].paused);
    assert!(matches!(
        store
            .prepare_agent_start(
                TrustedPrincipal(&principal),
                "fixture-start",
                &json!({"agent_id": AGENT_ID})
            )
            .await?,
        AgentStartPlan::Outcome(_)
    ));
    assert!(store.assign_pending_turn("general").await?.is_none());
    store.abort_runtime_restart(OPERATION).await?;
    let resumed = store
        .assign_pending_turn("general")
        .await?
        .ok_or("queued input was lost")?;
    assert_eq!(resumed.next_assignments.len(), 1);
    assert_eq!(
        resumed.next_assignments[0].session.inflight_inputs[0].event_id,
        queued_event
    );
    assert!(
        matches!(store.prepare_runtime_restart(NEXT_OPERATION).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "runtime_restart_busy")
    );
    assert_eq!(
        store.runtime_restart_status().await?.map(|r| r.phase),
        Some(RuntimeRestartPhase::Aborted)
    );
    Ok(())
}

#[tokio::test]
async fn reopened_runtime_cannot_abort_previous_custody_or_ignore_corrupt_restart()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let record = store.prepare_runtime_restart(OPERATION).await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    assert_eq!(store.runtime_restart_status().await?, Some(record));
    assert!(matches!(store.abort_runtime_restart(OPERATION).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "runtime_restart_stale"));
    assert!(
        matches!(store.prepare_agent_start(TrustedPrincipal(&principal), "after-reopen", &json!({"agent_id": AGENT_ID})).await,
        Err(PersistenceError::CommandUnresolved { code, .. }) if code == "runtime_restarting")
    );
    sqlx::query("UPDATE runtime_metadata SET value = '{' WHERE key = 'runtime_restart_v1'")
        .execute(&store.pool)
        .await?;
    assert!(store.runtime_restart_status().await.is_err());
    assert!(store.prepare_runtime_restart(NEXT_OPERATION).await.is_err());
    assert!(store.assign_pending_turn("general").await.is_err());
    Ok(())
}

async fn start_local_fixture(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut transaction = store.pool.begin().await?;
    let mut session = super::load_session(&mut transaction, "general", AGENT_ID).await?;
    // The lifecycle fixture's placeholder queue is unrelated to actual room inputs.
    session.pending_inputs.clear();
    super::save_session(&mut transaction, &session).await?;
    transaction.commit().await?;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(TrustedPrincipal(principal), "fixture-start", &payload)
        .await?
    else {
        panic!("fixture must prepare a start")
    };
    assert!(matches!(store.prepare_runtime_restart(OPERATION).await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "runtime_restart_busy"));
    assert!(store.runtime_restart_status().await?.is_none());
    let started = AgentRuntimeStarted {
        runtime_handle_id: "local-fixture-handle".to_owned(),
        runtime_owner_id: "local-fixture-owner".to_owned(),
        runtime_lease_token: "local-fixture-lease".to_owned(),
        provider_session_id: "local-fixture-session".to_owned(),
        runtime_reused: false,
        provider_session_reused: false,
        provider_session_active: true,
    };
    store
        .authorize_agent_start_effect(
            TrustedPrincipal(principal),
            "fixture-start",
            &payload,
            &effect.operation_id,
            "agent.start",
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await?;
    store
        .complete_agent_start(
            principal,
            "fixture-start",
            &payload,
            &effect.operation_id,
            &started,
        )
        .await?;
    Ok(())
}

async fn queue_local_fixture(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut transaction = store.pool.begin().await?;
    let session = super::load_session(&mut transaction, "general", AGENT_ID).await?;
    transaction.commit().await?;
    let runtime = crate::AgentResidentRuntime {
        runtime_handle_id: session.runtime_handle_id,
        runtime_owner_id: session.runtime_owner_id,
        runtime_lease_token: session.runtime_lease_token,
        runtime_profile_key: session.runtime_profile_key,
    };
    let payload = json!({"agent_id": AGENT_ID});
    store
        .execute_agent_pause(
            TrustedPrincipal(principal),
            "fixture-pause",
            &payload,
            &runtime,
        )
        .await?;
    let message = store
        .execute_message_with_turn(
            principal,
            "fixture-queued",
            "message.send",
            &json!({"content":"@Terra reply after resume"}),
        )
        .await?;
    assert!(message.assignments.is_empty());
    store
        .resume_paused_agent(
            TrustedPrincipal(principal),
            "fixture-resume",
            &payload,
            &runtime,
        )
        .await?
        .ok_or("fixture was not paused")?;
    Ok(message.outcome.event.id)
}
