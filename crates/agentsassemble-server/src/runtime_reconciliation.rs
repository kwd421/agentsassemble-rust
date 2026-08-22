use agentsassemble_persistence::{PersistenceError, RuntimeReconciliationObservation, SqliteStore};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeGone, ProviderRuntimeObservation};
use futures_util::future::join_all;

/// Reconciles every durable runtime candidate before network admission.
///
/// # Errors
///
/// Returns a persistence error if an exact observed transition cannot commit.
pub async fn reconcile_runtime_ownership(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
) -> Result<usize, PersistenceError> {
    let candidates = store.load_runtime_reconciliation_candidates().await?;
    let observations = join_all(
        candidates
            .iter()
            .map(|candidate| provider_adapter.observe(&candidate.session)),
    )
    .await;
    for (candidate, observation) in candidates.iter().zip(observations) {
        let observation = persistence_observation(observation);
        store
            .apply_runtime_reconciliation(candidate, &observation)
            .await?;
    }
    Ok(candidates.len())
}

fn persistence_observation(
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

pub(crate) async fn checkpoint_confirmed_shutdowns(
    store: &SqliteStore,
    gone: &[ProviderRuntimeGone],
) -> Result<(), PersistenceError> {
    if gone.is_empty() {
        return Ok(());
    }
    let candidates = store.load_runtime_reconciliation_candidates().await?;
    for stopped in gone {
        let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.session.public.room_id == stopped.room_id
                && candidate.session.public.session_id == stopped.session_id
        }) else {
            continue;
        };
        let durable_identity_is_empty = candidate.session.runtime_handle_id.is_empty()
            && candidate.session.runtime_owner_id.is_empty();
        let durable_identity_matches = candidate.session.runtime_handle_id
            == stopped.runtime_handle_id
            && candidate.session.runtime_owner_id == stopped.runtime_owner_id;
        if !durable_identity_is_empty && !durable_identity_matches {
            return Err(PersistenceError::CommandRejected {
                code: "stale_reconciliation_candidate",
                message: "Confirmed shutdown no longer matches durable runtime authority."
                    .to_owned(),
            });
        }
        store
            .apply_runtime_reconciliation(candidate, &RuntimeReconciliationObservation::Gone)
            .await?;
    }
    Ok(())
}
