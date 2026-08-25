use agentsassemble_persistence::{
    AgentTurnAssignment, PersistenceError, RuntimeReconciliationCandidate,
    RuntimeReconciliationObservation, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeObservation};

use crate::RoomRuntime;

pub(super) async fn recover_startup_observed_runtime(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    candidate: &RuntimeReconciliationCandidate,
    observation: ProviderRuntimeObservation,
) -> Result<Vec<AgentTurnAssignment>, PersistenceError> {
    let assignments = store
        .apply_runtime_reconciliation(candidate, &persistence_observation(observation))
        .await?;
    if !assignments.is_empty() {
        release_checkpointed_absence(provider_adapter, candidate).await;
        return Ok(assignments);
    }
    let room_id = &candidate.session.public.room_id;
    let session_id = &candidate.session.public.session_id;
    let Some(current) = store
        .load_runtime_reconciliation_candidate(room_id, session_id)
        .await?
    else {
        return Ok(Vec::new());
    };
    let handle_id = &current.session.runtime_handle_id;
    let owner_id = &current.session.runtime_owner_id;
    let lease_token = &current.session.runtime_lease_token;
    if provider_adapter
        .stop(room_id, session_id, handle_id, owner_id, lease_token)
        .await
        .is_err()
    {
        return Ok(Vec::new());
    }
    commit_startup_gone(store, provider_adapter, &current).await
}

pub(super) async fn commit_startup_gone(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<Vec<AgentTurnAssignment>, PersistenceError> {
    let assignments = store
        .apply_runtime_reconciliation(candidate, &RuntimeReconciliationObservation::Gone)
        .await?;
    release_checkpointed_absence(provider_adapter, candidate).await;
    Ok(assignments)
}

pub(super) async fn recover_dynamic_observed_runtime(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &RuntimeReconciliationCandidate,
    observation: ProviderRuntimeObservation,
) {
    let room_id = &candidate.session.public.room_id;
    let session_id = &candidate.session.public.session_id;
    match store
        .apply_runtime_reconciliation(candidate, &persistence_observation(observation))
        .await
    {
        Ok(assignments) if !assignments.is_empty() => {
            release_checkpointed_absence(provider_adapter, candidate).await;
            if let Err(error) = rooms
                .publish_then_resume_assigned_turns(room_id, assignments)
                .await
            {
                tracing::warn!(
                    %error,
                    %room_id,
                    "recovered ordered-floor assignment remains durable for retry"
                );
            }
            return;
        }
        Ok(_) => {}
        Err(error) if stale_candidate(&error) => return,
        Err(error) => {
            tracing::warn!(
                %error,
                %room_id,
                %session_id,
                "server-owned lifecycle recovery could not checkpoint runtime authority"
            );
            return;
        }
    }
    let current = match store
        .load_runtime_reconciliation_candidate(room_id, session_id)
        .await
    {
        Ok(Some(current)) => current,
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(
                %error,
                %room_id,
                %session_id,
                "server-owned lifecycle recovery could not reload runtime authority"
            );
            return;
        }
    };
    let handle_id = &current.session.runtime_handle_id;
    let owner_id = &current.session.runtime_owner_id;
    let lease_token = &current.session.runtime_lease_token;
    if let Err(error) = provider_adapter
        .stop(room_id, session_id, handle_id, owner_id, lease_token)
        .await
    {
        tracing::warn!(
            code = error.code,
            %room_id,
            %session_id,
            "server-owned lifecycle recovery could not quiesce an observed runtime"
        );
        return;
    }
    commit_and_publish_gone(store, provider_adapter, rooms, &current).await;
}

pub(super) async fn commit_and_publish_gone(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    candidate: &RuntimeReconciliationCandidate,
) -> bool {
    let room_id = candidate.session.public.room_id.clone();
    let session_id = candidate.session.public.session_id.clone();
    match commit_dynamic_gone(store, candidate).await {
        Ok(Some(assignments)) => {
            release_checkpointed_absence(provider_adapter, candidate).await;
            match rooms
                .publish_then_resume_assigned_turns(&room_id, assignments)
                .await
            {
                Ok(()) => true,
                Err(error) => {
                    tracing::warn!(
                        %error,
                        %room_id,
                        "recovered ordered-floor assignment remains durable for retry"
                    );
                    false
                }
            }
        }
        Ok(None) => false,
        Err(error) => {
            tracing::warn!(
                %error,
                %room_id,
                session_id,
                "server-owned lifecycle recovery observation could not commit"
            );
            false
        }
    }
}

pub(super) async fn release_checkpointed_absence(
    provider_adapter: &ProviderAdapter,
    candidate: &RuntimeReconciliationCandidate,
) {
    match candidate.session.lifecycle_intent_action.as_str() {
        "start" => {
            provider_adapter
                .release_checkpointed_start_absence(&candidate.session)
                .await;
        }
        "" | "stop" => {
            provider_adapter
                .release_confirmed_stop(
                    &candidate.session.public.room_id,
                    &candidate.session.public.session_id,
                    &candidate.session.runtime_handle_id,
                    &candidate.session.runtime_owner_id,
                    &candidate.session.runtime_lease_token,
                )
                .await;
        }
        _ => unreachable!("checkpointed lifecycle absence must be start or stop"),
    }
}

pub(super) async fn commit_dynamic_gone(
    store: &SqliteStore,
    candidate: &RuntimeReconciliationCandidate,
) -> Result<Option<Vec<AgentTurnAssignment>>, PersistenceError> {
    match store
        .apply_runtime_reconciliation(candidate, &RuntimeReconciliationObservation::Gone)
        .await
    {
        Ok(assignments) => Ok(Some(assignments)),
        Err(error) if stale_candidate(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

pub(super) fn stale_candidate(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::CommandRejected {
            code: "stale_reconciliation_candidate",
            ..
        }
    )
}

pub(super) fn persistence_observation(
    observation: ProviderRuntimeObservation,
) -> RuntimeReconciliationObservation {
    match observation {
        ProviderRuntimeObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        } => RuntimeReconciliationObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        },
        ProviderRuntimeObservation::Gone => RuntimeReconciliationObservation::Gone,
        ProviderRuntimeObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        } => RuntimeReconciliationObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        },
        ProviderRuntimeObservation::Ambiguous { reason_code } => {
            RuntimeReconciliationObservation::Ambiguous { reason_code }
        }
    }
}
