use agentsassemble_persistence::{PersistenceError, RuntimeReconciliationObservation, SqliteStore};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeObservation};

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
    for candidate in &candidates {
        let observation = match provider_adapter.observe(&candidate.session).await {
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
        };
        store
            .apply_runtime_reconciliation(candidate, &observation)
            .await?;
    }
    Ok(candidates.len())
}
