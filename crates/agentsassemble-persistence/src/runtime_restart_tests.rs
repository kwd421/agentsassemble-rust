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
async fn retired_restart_receipt_cannot_replace_current_admission_owner()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, _, _directory) = fixture().await;
    store.prepare_runtime_restart(OPERATION).await?;
    let retired = store.abort_runtime_restart(OPERATION).await?;
    let current = store.prepare_runtime_restart(NEXT_OPERATION).await?;
    assert_eq!(store.prepare_runtime_restart(OPERATION).await?, retired);
    assert_eq!(
        store.runtime_restart_operation(OPERATION).await?,
        Some(retired)
    );
    assert_eq!(store.runtime_restart_status().await?, Some(current));
    Ok(())
}

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
    assert!(
        store
            .fail_abandoned_runtime_restart_after_cleanup()
            .await
            .is_err()
    );
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
    assert!(store.fail_abandoned_runtime_restart_after_cleanup().await?);
    assert!(!store.fail_abandoned_runtime_restart_after_cleanup().await?);
    assert_eq!(
        store
            .runtime_restart_status()
            .await?
            .map(|record| record.phase),
        Some(RuntimeRestartPhase::Failed)
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
    let runtime = pause_local_fixture(store, principal).await?;
    let payload = json!({"agent_id": AGENT_ID});
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

#[tokio::test]
async fn replacement_reconstructs_only_captured_targets_and_preserves_pause()
-> Result<(), Box<dyn std::error::Error>> {
    use agentsassemble_domain::AgentRuntimeStatus;
    for paused in [false, true] {
        let (store, principal, directory) = fixture().await;
        start_local_fixture(&store, &principal).await?;
        if paused {
            pause_local_fixture(&store, &principal).await?;
        }
        assert_eq!(
            store.prepare_runtime_restart(OPERATION).await?.targets[0].paused,
            paused
        );
        store
            .begin_runtime_restart_drain(OPERATION, "candidate-image")
            .await?;
        assert!(store.abort_runtime_restart(OPERATION).await.is_err());
        assert!(
            store
                .begin_runtime_restart_recovery(OPERATION, "candidate-image")
                .await
                .is_err()
        );
        assert!(
            store
                .fail_runtime_restart_after_cleanup(OPERATION)
                .await
                .is_err()
        );
        stop_restart_fixture(&store).await?;
        store.pool.close().await;
        drop(store);
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
        assert!(
            store
                .begin_runtime_restart_recovery(OPERATION, "wrong-image")
                .await
                .is_err()
        );
        store
            .begin_runtime_restart_recovery(OPERATION, "candidate-image")
            .await?;
        assert!(store.complete_runtime_restart(OPERATION).await.is_err());
        assert!(
            store
                .runtime_restart_target(OPERATION, "general", "unrelated")
                .await
                .is_err()
        );
        reconstruct_local_fixture(&store, paused).await?;
        assert!(store.assign_pending_turn("general").await?.is_none());
        let complete = store.complete_runtime_restart(OPERATION).await?;
        assert_eq!(complete.phase, RuntimeRestartPhase::Completed);
        assert_eq!(store.prepare_runtime_restart(OPERATION).await?, complete);
        let session = store
            .snapshot("general", 0, 200)
            .await?
            .agent_sessions
            .remove(0);
        assert_eq!(
            session.runtime_status,
            if paused {
                AgentRuntimeStatus::Paused
            } else {
                AgentRuntimeStatus::Idle
            }
        );
        assert_eq!(session.enabled, !paused);
        assert!(
            store
                .fail_runtime_restart_after_cleanup(OPERATION)
                .await
                .is_err()
        );
        stop_restart_fixture(&store).await?;
        assert_eq!(
            store
                .fail_runtime_restart_after_cleanup(OPERATION)
                .await?
                .phase,
            RuntimeRestartPhase::Failed
        );
    }
    Ok(())
}

#[tokio::test]
async fn interrupted_reconstruction_uses_existing_custody_cleanup_before_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    start_local_fixture(&store, &principal).await?;
    store.prepare_runtime_restart(OPERATION).await?;
    store
        .begin_runtime_restart_drain(OPERATION, "candidate-image")
        .await?;
    stop_restart_fixture(&store).await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    store
        .begin_runtime_restart_recovery(OPERATION, "candidate-image")
        .await?;
    store
        .authorize_runtime_restart_target(
            OPERATION,
            &crate::RuntimeRestartTarget {
                room_id: "general".into(),
                session_id: AGENT_ID.into(),
                paused: false,
            },
            "handle",
            "owner",
            "lease",
        )
        .await?;
    let captured = store
        .load_runtime_reconciliation_candidate("general", AGENT_ID)
        .await?
        .ok_or("reconstruction custody missing")?;
    store
        .apply_runtime_shutdown_reconciliation(
            &captured,
            &crate::RuntimeReconciliationObservation::LeaseUncertain {
                handle_id: captured.session.runtime_handle_id.clone(),
                owner_id: captured.session.runtime_owner_id.clone(),
                reason_code: "provider_stop_unconfirmed".to_owned(),
            },
        )
        .await?;
    for _ in 0..2 {
        assert!(
            store
                .fail_runtime_restart_after_cleanup(OPERATION)
                .await
                .is_err()
        );
        let retained = store
            .load_runtime_reconciliation_candidate("general", AGENT_ID)
            .await?
            .ok_or("unconfirmed custody was erased")?;
        assert_eq!(
            retained.session.runtime_handle_id,
            captured.session.runtime_handle_id
        );
        assert_eq!(
            retained.session.runtime_owner_id,
            captured.session.runtime_owner_id
        );
        assert_eq!(
            retained.session.runtime_lease_token,
            captured.session.runtime_lease_token
        );
        assert!(retained.session.public.recovery_required);
        assert_eq!(
            store
                .runtime_restart_status()
                .await?
                .map(|record| record.phase),
            Some(RuntimeRestartPhase::Recovering)
        );
    }
    stop_restart_fixture(&store).await?;
    assert_eq!(
        store
            .fail_runtime_restart_after_cleanup(OPERATION)
            .await?
            .phase,
        RuntimeRestartPhase::Failed
    );
    assert!(matches!(
        store
            .prepare_agent_start(
                TrustedPrincipal(&principal),
                "retry-after-failed-restart",
                &json!({"agent_id": AGENT_ID})
            )
            .await?,
        AgentStartPlan::Start(_)
    ));
    Ok(())
}

async fn stop_restart_fixture(store: &SqliteStore) -> Result<(), Box<dyn std::error::Error>> {
    let candidate = store
        .load_runtime_reconciliation_candidate("general", AGENT_ID)
        .await?
        .ok_or("fixture runtime custody missing")?;
    store
        .apply_runtime_shutdown_reconciliation(
            &candidate,
            &crate::RuntimeReconciliationObservation::Gone,
        )
        .await?;
    Ok(())
}

async fn pause_local_fixture(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
) -> Result<crate::AgentResidentRuntime, Box<dyn std::error::Error>> {
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
    Ok(runtime)
}

async fn reconstruct_local_fixture(
    store: &SqliteStore,
    paused: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let old = store
        .runtime_restart_target(OPERATION, "general", AGENT_ID)
        .await?;
    let started = AgentRuntimeStarted {
        runtime_handle_id: "restart-handle".into(),
        runtime_owner_id: "restart-owner".into(),
        runtime_lease_token: "restart-lease".into(),
        provider_session_id: old.provider_session_id,
        runtime_reused: false,
        provider_session_reused: true,
        provider_session_active: true,
    };
    assert!(
        store
            .complete_runtime_restart_target(OPERATION, "general", AGENT_ID, &started)
            .await
            .is_err()
    );
    store
        .authorize_runtime_restart_target(
            OPERATION,
            &crate::RuntimeRestartTarget {
                room_id: "general".into(),
                session_id: AGENT_ID.into(),
                paused,
            },
            &started.runtime_handle_id,
            &started.runtime_owner_id,
            &started.runtime_lease_token,
        )
        .await?;
    assert!(
        store
            .runtime_restart_target(OPERATION, "general", AGENT_ID)
            .await
            .is_err()
    );
    assert!(
        store
            .fail_runtime_restart_after_cleanup(OPERATION)
            .await
            .is_err()
    );
    let candidate = store
        .load_runtime_reconciliation_candidate("general", AGENT_ID)
        .await?
        .ok_or("restart custody disappeared")?;
    assert!(
        candidate.reservation.is_none(),
        "restart must not fabricate a human command"
    );
    assert_eq!(
        candidate.session.runtime_handle_id,
        started.runtime_handle_id
    );
    store
        .complete_runtime_restart_target(OPERATION, "general", AGENT_ID, &started)
        .await?;
    Ok(())
}
