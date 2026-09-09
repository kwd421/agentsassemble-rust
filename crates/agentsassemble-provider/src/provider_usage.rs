use std::{collections::BTreeMap, sync::Arc};

use agentsassemble_domain::{ProviderQuota, ProviderUsage};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared, join_all},
};
use tokio::{sync::Mutex, task::AbortHandle};
use tokio_util::sync::CancellationToken;

use crate::{ProviderCredentialStore, registration::provider_registration_by_id};

pub(crate) type ProviderUsageReader =
    for<'a> fn(
        &'a ProviderCredentialStore,
        &'a CancellationToken,
    ) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderUsageError {
    #[error("This provider does not support account usage.")]
    Unsupported,
    #[error("The provider account credential is missing.")]
    Missing,
    #[error("The provider credential store is unavailable.")]
    CredentialUnavailable,
    #[error("The provider rejected account usage authorization.")]
    Authentication,
    #[error("Provider usage timed out.")]
    Timeout,
    #[error("Provider usage was cancelled.")]
    Cancelled,
    #[error("Provider usage is unavailable.")]
    Unavailable,
    #[error("The provider returned invalid usage data.")]
    InvalidResponse,
    #[error("Provider usage process cleanup could not be confirmed.")]
    CleanupUnconfirmed,
}

impl From<crate::process::ProbeFailure> for ProviderUsageError {
    fn from(error: crate::process::ProbeFailure) -> Self {
        use crate::process::ProbeFailure as E;
        match error {
            E::Missing => Self::Missing,
            E::Timeout => Self::Timeout,
            E::Authentication => Self::Authentication,
            E::Malformed | E::CatalogTooLarge => Self::InvalidResponse,
            E::Failed => Self::Unavailable,
            E::Cancelled => Self::Cancelled,
            E::CleanupUnconfirmed => Self::CleanupUnconfirmed,
        }
    }
}

type UsageResult = Shared<BoxFuture<'static, Result<ProviderUsage, ProviderUsageError>>>;

struct UsageRun {
    task: AbortHandle,
    result: UsageResult,
}

struct UsageOwner {
    credentials: ProviderCredentialStore,
    cancellation: CancellationToken,
    runs: Mutex<BTreeMap<&'static str, UsageRun>>,
}

impl Drop for UsageOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Clone)]
pub struct ProviderUsageService(Arc<UsageOwner>);

impl ProviderUsageService {
    #[must_use]
    pub fn new(credentials: ProviderCredentialStore, cancellation: CancellationToken) -> Self {
        Self(Arc::new(UsageOwner {
            credentials,
            cancellation,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }

    /// Reads account quota on demand, coalescing concurrent requests without caching old results.
    ///
    /// # Errors
    /// Returns typed dependency, account, transport, cancellation or cleanup failures.
    pub async fn read(&self, provider_id: &str) -> Result<ProviderUsage, ProviderUsageError> {
        let registration =
            provider_registration_by_id(provider_id).ok_or(ProviderUsageError::Unsupported)?;
        let reader = registration.usage.ok_or(ProviderUsageError::Unsupported)?;
        self.read_with(registration.id, reader).await
    }

    async fn read_with(
        &self,
        id: &'static str,
        reader: ProviderUsageReader,
    ) -> Result<ProviderUsage, ProviderUsageError> {
        let result = {
            let mut runs = self.0.runs.lock().await;
            if self.0.cancellation.is_cancelled() {
                return Err(ProviderUsageError::Cancelled);
            }
            // Shared completion depends on its consumers. A lost response must not
            // turn a finished native read into a cached result for the next request.
            let inflight = if let Some(run) = runs.get(id) {
                if run.task.is_finished() {
                    if run.result.clone().await == Err(ProviderUsageError::CleanupUnconfirmed) {
                        return Err(ProviderUsageError::CleanupUnconfirmed);
                    }
                    None
                } else {
                    Some(run.result.clone())
                }
            } else {
                None
            };
            if let Some(result) = inflight {
                result
            } else {
                let credentials = self.0.credentials.clone();
                let cancellation = self.0.cancellation.child_token();
                let task = tokio::spawn(async move {
                    let quota = reader(&credentials, &cancellation).await?;
                    Ok(ProviderUsage {
                        provider_id: id.to_owned(),
                        observed_at: chrono::Utc::now(),
                        quota,
                    })
                });
                let completion = task.abort_handle();
                let result = async move {
                    task.await
                        .map_err(|_| ProviderUsageError::CleanupUnconfirmed)?
                }
                .boxed()
                .shared();
                runs.insert(
                    id,
                    UsageRun {
                        task: completion,
                        result: result.clone(),
                    },
                );
                result
            }
        };
        result.await
    }

    /// Cancels and joins every in-flight account read.
    ///
    /// # Errors
    /// Reports a task failure or unconfirmed native process cleanup.
    pub async fn shutdown(&self) -> Result<(), ProviderUsageError> {
        self.0.cancellation.cancel();
        let runs = std::mem::take(&mut *self.0.runs.lock().await);
        let results = join_all(runs.into_values().map(|run| run.result)).await;
        if results.contains(&Err(ProviderUsageError::CleanupUnconfirmed)) {
            return Err(ProviderUsageError::CleanupUnconfirmed);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;

    static CALLS: AtomicUsize = AtomicUsize::new(0);
    static ENTERED: Notify = Notify::const_new();
    static RELEASE: Notify = Notify::const_new();
    static COMPLETED: Notify = Notify::const_new();

    fn controlled<'a>(
        _: &'a ProviderCredentialStore,
        cancellation: &'a CancellationToken,
    ) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
        async move {
            CALLS.fetch_add(1, Ordering::SeqCst);
            ENTERED.notify_one();
            let result = tokio::select! {
                () = cancellation.cancelled() => Err(ProviderUsageError::Cancelled),
                () = RELEASE.notified() => Ok(ProviderQuota::Balance { is_available: false, balances: vec![] }),
            };
            COMPLETED.notify_one();
            result
        }.boxed()
    }

    #[tokio::test(flavor = "current_thread")]
    async fn reads_coalesce_refresh_after_response_loss_and_join_on_shutdown() {
        let service = ProviderUsageService::new(
            ProviderCredentialStore::isolated_test_store(),
            CancellationToken::new(),
        );
        let first = service.read_with("deepseek", controlled);
        let retry = async {
            ENTERED.notified().await;
            let retry = service.read_with("deepseek", controlled);
            tokio::pin!(retry);
            assert!(futures_util::poll!(retry.as_mut()).is_pending());
            RELEASE.notify_one();
            retry.await
        };
        let (first, retry) = tokio::join!(first, retry);
        assert_eq!(first, retry);
        assert!(first.is_ok());
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        COMPLETED.notified().await;
        let mut abandoned = Box::pin(service.read_with("deepseek", controlled));
        assert!(futures_util::poll!(abandoned.as_mut()).is_pending());
        ENTERED.notified().await;
        drop(abandoned);
        RELEASE.notify_one();
        // This current-thread task runs through completion before the waiter resumes.
        COMPLETED.notified().await;
        let mut next = Box::pin(service.read_with("deepseek", controlled));
        assert!(futures_util::poll!(next.as_mut()).is_pending());
        let shutdown = async {
            ENTERED.notified().await;
            service.shutdown().await
        };
        let (next, shutdown) = tokio::join!(next, shutdown);
        assert_eq!(next, Err(ProviderUsageError::Cancelled));
        assert_eq!(shutdown, Ok(()));
        assert_eq!(CALLS.load(Ordering::SeqCst), 3);
        assert_eq!(
            service.read("deepseek").await,
            Err(ProviderUsageError::Cancelled)
        );
        assert_eq!(
            service.read("antigravity").await,
            Err(ProviderUsageError::Unsupported)
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn response_loss_preserves_completed_cleanup_failure() {
        static ENTERED: Notify = Notify::const_new();
        static RELEASE: Notify = Notify::const_new();
        static COMPLETED: Notify = Notify::const_new();
        fn failed_cleanup<'a>(
            _: &'a ProviderCredentialStore,
            _: &'a CancellationToken,
        ) -> BoxFuture<'a, Result<ProviderQuota, ProviderUsageError>> {
            async {
                ENTERED.notify_one();
                RELEASE.notified().await;
                COMPLETED.notify_one();
                Err(ProviderUsageError::CleanupUnconfirmed)
            }
            .boxed()
        }
        let service = ProviderUsageService::new(
            ProviderCredentialStore::isolated_test_store(),
            CancellationToken::new(),
        );
        let mut abandoned = Box::pin(service.read_with("deepseek", failed_cleanup));
        assert!(futures_util::poll!(abandoned.as_mut()).is_pending());
        ENTERED.notified().await;
        drop(abandoned);
        RELEASE.notify_one();
        COMPLETED.notified().await;
        let next = service.read_with("deepseek", failed_cleanup);
        tokio::pin!(next);
        assert_eq!(
            futures_util::poll!(next.as_mut()),
            std::task::Poll::Ready(Err(ProviderUsageError::CleanupUnconfirmed))
        );
        assert_eq!(
            service.shutdown().await,
            Err(ProviderUsageError::CleanupUnconfirmed)
        );
    }
}
