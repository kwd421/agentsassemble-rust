//! The room actor consumes an exact bridge failure; durable lifecycle remains authoritative.
use agentsassemble_persistence::{PersistenceError, SqliteStore};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeFailure};

pub(super) async fn record(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    room_id: &str,
    failure: &ProviderRuntimeFailure,
) -> Result<(), PersistenceError> {
    if let Some(candidate) = store
        .load_runtime_reconciliation_candidate(room_id, failure.session_id())
        .await?
        && failure.matches(&candidate.session)
    {
        store.record_idle_runtime_failure(&candidate).await?;
    }
    // A blocking execution has its own provider-result owner; retired generations
    // and in-flight lifecycle commands cannot be changed by this notification.
    adapter.acknowledge_runtime_failure(failure).await;
    Ok(())
}
