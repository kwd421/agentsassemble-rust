use agentsassemble_domain::{AgentLifecycleIntentStatus, DurableAgentSession};
use agentsassemble_persistence::{
    AgentTurnCommit, PersistenceError, RoomRuntimeCleanupKey, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeObservation};
use futures_util::{StreamExt, stream};
use tokio_util::sync::CancellationToken;

use crate::{
    RoomRuntime,
    room_command_execution::persistence_error_code,
    runtime_reconciliation::{LIVE_OBSERVATION_TIMEOUT, RECOVERY_OBSERVATION_CONCURRENCY},
    runtime_reconciliation_cleanup::release_checkpointed_absence,
};

/// Advances pending removal using the existing exact turn and lifecycle receipt owners.
/// `Some` means durable progress, not that every cleanup row has completed.
pub(crate) async fn attempt_cleanup(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    key: &RoomRuntimeCleanupKey,
) -> Result<Option<AgentTurnCommit>, PersistenceError> {
    if let Some(commit) = store.finish_room_runtime_cleanup(key).await? {
        return Ok(Some(commit));
    }
    if let Some(candidate) = store.load_room_runtime_cleanup_turn(key).await? {
        observe_then_stop(adapter, &candidate.session).await?;
        store
            .finalize_provider_turn_runtime_gone_for_shutdown(&candidate)
            .await?;
        adapter
            .release_confirmed_stop(
                &key.room_id,
                &key.session_id,
                &candidate.session.runtime_handle_id,
                &candidate.session.runtime_owner_id,
                &candidate.session.runtime_lease_token,
            )
            .await;
    } else if let Some(candidate) = store.load_room_runtime_cleanup_candidate(key).await? {
        if candidate.session.lifecycle_intent_status == AgentLifecycleIntentStatus::Prepared {
            store
                .reject_abandoned_lifecycle_before_effect(&candidate)
                .await?;
        } else {
            if candidate.session.lifecycle_intent_status
                != AgentLifecycleIntentStatus::EffectApplied
            {
                observe_then_stop(adapter, &candidate.session).await?;
            }
            store
                .apply_runtime_shutdown_reconciliation(
                    &candidate,
                    &agentsassemble_persistence::RuntimeReconciliationObservation::Gone,
                )
                .await?;
            release_checkpointed_absence(adapter, &candidate).await;
        }
    } else {
        return Ok(None);
    }
    Ok(Some(
        store
            .finish_room_runtime_cleanup(key)
            .await?
            .unwrap_or(AgentTurnCommit {
                events: Vec::new(),
                next_assignments: Vec::new(),
            }),
    ))
}

async fn observe_then_stop(
    adapter: &ProviderAdapter,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let observation = tokio::time::timeout(LIVE_OBSERVATION_TIMEOUT, adapter.observe(session))
        .await
        .map_err(|_| unresolved("runtime_cleanup_observation_timeout"))?;
    match observation {
        ProviderRuntimeObservation::Gone => Ok(()),
        ProviderRuntimeObservation::Adopted { .. }
        | ProviderRuntimeObservation::LeaseUncertain { .. } => adapter
            .stop(
                &session.public.room_id,
                &session.public.session_id,
                &session.runtime_handle_id,
                &session.runtime_owner_id,
                &session.runtime_lease_token,
            )
            .await
            .map_err(|error| unresolved(error.code)),
        ProviderRuntimeObservation::Ambiguous { .. } => {
            Err(unresolved("runtime_cleanup_unconfirmed"))
        }
    }
}

pub(crate) async fn reconcile_cleanup_page(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    cursor: Option<&RoomRuntimeCleanupKey>,
    cancellation: &CancellationToken,
) -> Result<Option<RoomRuntimeCleanupKey>, PersistenceError> {
    let page = store.load_room_runtime_cleanup_page(cursor).await?;
    stream::iter(page.keys)
        .for_each_concurrent(RECOVERY_OBSERVATION_CONCURRENCY, |key| async move {
            if cancellation.is_cancelled() {
                return;
            }
            match Box::pin(attempt_cleanup(store, adapter, &key)).await {
                Ok(Some(commit)) => {
                    rooms.notify_room_publication(&key.room_id).await;
                    if let Err(error) = rooms
                        .publish_then_resume_assigned_turns(&key.room_id, commit.next_assignments)
                        .await
                    {
                        log_pending(&key, &error);
                    }
                }
                Ok(None) => {}
                Err(error) => log_pending(&key, &error),
            }
        })
        .await;
    Ok(page.next_cursor)
}

pub(crate) async fn reconcile_before_admission(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    rooms: &RoomRuntime,
    cancellation: &CancellationToken,
) -> Result<(), PersistenceError> {
    let mut cursor = None;
    loop {
        cursor =
            reconcile_cleanup_page(store, adapter, rooms, cursor.as_ref(), cancellation).await?;
        if cursor.is_none() || cancellation.is_cancelled() {
            break;
        }
    }
    let mut deletion_cursor = None;
    loop {
        deletion_cursor =
            reconcile_deletion_page(store, rooms, deletion_cursor.as_deref(), cancellation).await?;
        if deletion_cursor.is_none() || cancellation.is_cancelled() {
            return Ok(());
        }
    }
}

pub(crate) async fn reconcile_deletion_page(
    store: &SqliteStore,
    rooms: &RoomRuntime,
    cursor: Option<&str>,
    cancellation: &CancellationToken,
) -> Result<Option<String>, PersistenceError> {
    let page = store.pending_room_deletions(cursor).await?;
    stream::iter(page.room_ids).for_each_concurrent(RECOVERY_OBSERVATION_CONCURRENCY, |room_id| async move {
        tokio::select! {
            () = cancellation.cancelled() => {},
            result = rooms.finalize_room_deletion(&room_id) => {
                if let Err(error) = result {
                    tracing::warn!(code = persistence_error_code(&error), room_id, "room deletion remains durable and pending");
                }
            }
        }
    }).await;
    Ok(page.next_cursor)
}

pub(crate) fn log_pending(key: &RoomRuntimeCleanupKey, error: &PersistenceError) {
    tracing::warn!(
        code = persistence_error_code(error),
        room_id = key.room_id,
        session_id = key.session_id,
        "removed runtime cleanup remains durable and pending"
    );
}

fn unresolved(code: impl Into<std::borrow::Cow<'static, str>>) -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: code.into(),
        message: "Participant access is revoked; exact provider cleanup is still pending."
            .to_owned(),
    }
}
