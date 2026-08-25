use std::time::Duration;

use agentsassemble_persistence::{
    AgentTurnCommit, PersistenceError, ProviderTurnExecutionPhase,
    ProviderTurnReconciliationCandidate, ProviderTurnReconciliationCursor,
    ProviderTurnStartAuthority, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderExactTurnAuthority, ProviderRuntimeObservation,
};
use futures_util::{StreamExt, stream};
use tokio_util::sync::CancellationToken;

use crate::RoomRuntime;

const LIVE_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
const RECOVERY_OBSERVATION_CONCURRENCY: usize = 8;

/// Reconciles every blocking provider turn before lifecycle recovery or network admission.
pub(crate) async fn reconcile_provider_turn_ownership(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
) -> Result<usize, PersistenceError> {
    let mut cursor: Option<ProviderTurnReconciliationCursor> = None;
    let mut reconciled = 0usize;
    loop {
        let page = store
            .load_provider_turn_reconciliation_page(cursor.as_ref())
            .await?;
        for candidate in page.candidates {
            reconcile_startup_candidate(store, provider_adapter, rooms, &candidate).await?;
            reconciled = reconciled.saturating_add(1);
        }
        let Some(next) = page.next_cursor else {
            break;
        };
        cursor = Some(next);
    }
    Ok(reconciled)
}

async fn reconcile_startup_candidate(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &ProviderTurnReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let execution = &candidate.execution;
    if let Some(effect) = &candidate.effect
        && let Some(commit) =
            crate::participant_mute_runtime::resume_exact_interrupt(store, provider_adapter, effect)
                .await?
    {
        publish_commit(rooms, commit).await?;
        return Ok(());
    }
    if execution.phase == ProviderTurnExecutionPhase::Assigned && candidate.effect.is_none() {
        match provider_adapter.observe(&candidate.session).await {
            ProviderRuntimeObservation::Gone => {
                finalize_gone(store, provider_adapter, rooms, candidate).await?;
            }
            ProviderRuntimeObservation::Adopted {
                handle_id,
                previous_owner_id,
                new_owner_id,
                runtime_profile_key,
            } if handle_id == execution.runtime_handle_id
                && previous_owner_id == execution.runtime_owner_id
                && new_owner_id == execution.runtime_owner_id
                && runtime_profile_key == candidate.session.runtime_profile_key =>
            {
                let assignment = store.recover_assigned_provider_turn(candidate).await?;
                rooms
                    .publish_then_resume_assigned_turns(&execution.room_id, vec![assignment])
                    .await?;
            }
            ProviderRuntimeObservation::Adopted { .. }
            | ProviderRuntimeObservation::LeaseUncertain { .. }
            | ProviderRuntimeObservation::Ambiguous { .. } => {}
        }
        return Ok(());
    }
    if matches!(
        provider_adapter.observe(&candidate.session).await,
        ProviderRuntimeObservation::Gone
    ) {
        finalize_gone(store, provider_adapter, rooms, candidate).await?;
        return Ok(());
    }
    if blocking_task_phase(execution.phase) {
        let commit = store
            .record_provider_turn_task_death(
                &execution.room_id,
                &execution.session_id,
                execution.turn_generation,
                &execution.execution_id,
            )
            .await?;
        publish_commit(rooms, commit).await?;
    }
    Ok(())
}

pub(crate) async fn reconcile_live_page(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidates: Vec<ProviderTurnReconciliationCandidate>,
    cancellation: &CancellationToken,
) {
    stream::iter(candidates)
        .for_each_concurrent(RECOVERY_OBSERVATION_CONCURRENCY, |candidate| async move {
            if cancellation.is_cancelled() {
                return;
            }
            if let Err(error) =
                reconcile_live_candidate(store, provider_adapter, rooms, &candidate).await
            {
                tracing::warn!(
                    %error,
                    room_id = %candidate.execution.room_id,
                    session_id = %candidate.execution.session_id,
                    turn_generation = candidate.execution.turn_generation,
                    "server-owned provider-turn recovery candidate remains unresolved"
                );
            }
        })
        .await;
}

async fn reconcile_live_candidate(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &ProviderTurnReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let authority = exact_authority(candidate);
    if let Some(effect) = &candidate.effect
        && let Some(commit) =
            crate::participant_mute_runtime::resume_exact_interrupt(store, provider_adapter, effect)
                .await?
    {
        publish_commit(rooms, commit).await?;
        return Ok(());
    }
    if let Some(result) = provider_adapter.retained_turn_result(&authority).await {
        let start = start_authority(candidate)?;
        let commit = crate::provider_turn::commit_exact_provider_result(
            store,
            provider_adapter,
            &start,
            result,
        )
        .await?;
        publish_commit(rooms, commit).await?;
        return Ok(());
    }
    if let Some(proof) = provider_adapter
        .retained_not_started_proof(&authority)
        .await
    {
        debug_assert_eq!(proof.exact_authority(), &authority);
        let commit = store.finalize_provider_turn_not_started(candidate).await?;
        provider_adapter.release_terminal_turn(&authority).await;
        publish_commit(rooms, commit).await?;
        return Ok(());
    }
    if provider_adapter.owns_exact_turn(&authority).await {
        return Ok(());
    }
    let observation = tokio::time::timeout(
        LIVE_OBSERVATION_TIMEOUT,
        provider_adapter.observe(&candidate.session),
    )
    .await
    .ok();
    reconcile_unowned_observation(store, provider_adapter, rooms, candidate, observation).await
}

async fn reconcile_unowned_observation(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &ProviderTurnReconciliationCandidate,
    observation: Option<ProviderRuntimeObservation>,
) -> Result<(), PersistenceError> {
    match observation {
        Some(ProviderRuntimeObservation::Gone) => {
            finalize_gone(store, provider_adapter, rooms, candidate).await?;
        }
        Some(ProviderRuntimeObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        }) if handle_id == candidate.execution.runtime_handle_id
            && previous_owner_id == candidate.execution.runtime_owner_id
            && new_owner_id == candidate.execution.runtime_owner_id
            && runtime_profile_key == candidate.session.runtime_profile_key =>
        {
            if candidate.execution.phase == ProviderTurnExecutionPhase::Assigned {
                let assignment = store.recover_assigned_provider_turn(candidate).await?;
                rooms
                    .publish_then_resume_assigned_turns(
                        &candidate.execution.room_id,
                        vec![assignment],
                    )
                    .await?;
            } else if blocking_task_phase(candidate.execution.phase) {
                let commit = store
                    .record_provider_turn_task_death(
                        &candidate.execution.room_id,
                        &candidate.execution.session_id,
                        candidate.execution.turn_generation,
                        &candidate.execution.execution_id,
                    )
                    .await?;
                publish_commit(rooms, commit).await?;
            }
        }
        Some(
            ProviderRuntimeObservation::Adopted { .. }
            | ProviderRuntimeObservation::LeaseUncertain { .. }
            | ProviderRuntimeObservation::Ambiguous { .. },
        )
        | None => {}
    }
    Ok(())
}

fn blocking_task_phase(phase: ProviderTurnExecutionPhase) -> bool {
    matches!(
        phase,
        ProviderTurnExecutionPhase::StartDispatching
            | ProviderTurnExecutionPhase::Running
            | ProviderTurnExecutionPhase::InterruptPending
            | ProviderTurnExecutionPhase::Quiescing
    )
}

fn start_authority(
    candidate: &ProviderTurnReconciliationCandidate,
) -> Result<ProviderTurnStartAuthority, PersistenceError> {
    let execution = &candidate.execution;
    if execution.start_dispatch_nonce.is_empty() {
        return Err(PersistenceError::CommandUnresolved {
            code: "provider_turn_start_unresolved",
            message: "The retained provider result has no durable start authority.".to_owned(),
        });
    }
    Ok(ProviderTurnStartAuthority {
        room_id: execution.room_id.clone(),
        session_id: execution.session_id.clone(),
        turn_generation: execution.turn_generation,
        execution_id: execution.execution_id.clone(),
        turn_id: execution.turn_id.clone(),
        runtime_handle_id: execution.runtime_handle_id.clone(),
        runtime_owner_id: execution.runtime_owner_id.clone(),
        runtime_lease_token: execution.runtime_lease_token.clone(),
        start_dispatch_nonce: execution.start_dispatch_nonce.clone(),
    })
}

fn exact_authority(candidate: &ProviderTurnReconciliationCandidate) -> ProviderExactTurnAuthority {
    let execution = &candidate.execution;
    ProviderExactTurnAuthority {
        room_id: execution.room_id.clone(),
        session_id: execution.session_id.clone(),
        execution_id: execution.execution_id.clone(),
        turn_id: execution.turn_id.clone(),
        turn_generation: execution.turn_generation,
        runtime_handle_id: execution.runtime_handle_id.clone(),
        runtime_owner_id: execution.runtime_owner_id.clone(),
        runtime_lease_token: execution.runtime_lease_token.clone(),
    }
}

async fn finalize_gone(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &ProviderTurnReconciliationCandidate,
) -> Result<(), PersistenceError> {
    let execution = &candidate.execution;
    let commit = store.finalize_provider_turn_runtime_gone(candidate).await?;
    provider_adapter
        .release_confirmed_stop(
            &execution.room_id,
            &execution.session_id,
            &execution.runtime_handle_id,
            &execution.runtime_owner_id,
            &execution.runtime_lease_token,
        )
        .await;
    publish_commit(rooms, commit).await
}

async fn publish_commit(
    rooms: &RoomRuntime,
    commit: AgentTurnCommit,
) -> Result<(), PersistenceError> {
    let Some(first_assignment) = commit.next_assignments.first() else {
        rooms.notify_committed_events(&commit.events).await;
        return Ok(());
    };
    let room_id = first_assignment.session.public.room_id.clone();
    if commit.events.iter().any(|event| event.room_id != room_id) {
        return Err(PersistenceError::CommandUnresolved {
            code: "provider_turn_recovery_authority_invalid",
            message: "Recovered provider events do not share assignment room authority.".to_owned(),
        });
    }
    rooms
        .publish_then_resume_assigned_turns(&room_id, commit.next_assignments)
        .await
}

#[cfg(test)]
#[path = "runtime_reconciliation_provider_replay_tests.rs"]
mod tests;
