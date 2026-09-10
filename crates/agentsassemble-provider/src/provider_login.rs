use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared, join_all},
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::provider_executable,
    process::{ProbeFailure, probe_with_timeout},
    registration::{ProviderRegistration, provider_registration_by_id},
};

type LoginEnvironment = fn() -> Result<Vec<(String, String)>, crate::runtime::DriverError>;

pub(crate) enum ProviderLoginFlow {
    BrowserOauth,
    InteractiveTerminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderLoginOutcome {
    Authenticated,
    Started,
}

pub(crate) struct ProviderLoginSpec {
    pub(crate) flow: ProviderLoginFlow,
    pub(crate) arguments: &'static [&'static str],
    pub(crate) environment: Option<LoginEnvironment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderLoginError {
    #[error("This provider does not support local login.")]
    Unsupported,
    #[error(
        "The terminal may have opened, but its launch could not be confirmed. Check it before retrying."
    )]
    HandoffUnconfirmed,
    #[error("The provider login executable is unavailable.")]
    Missing,
    #[error("Provider login timed out.")]
    Timeout,
    #[error("Provider login was cancelled.")]
    Cancelled,
    #[error("Provider login failed.")]
    Failed,
    #[error("Provider login process cleanup could not be confirmed.")]
    CleanupUnconfirmed,
    #[error("Login completed, but catalog refresh failed. Refresh the catalog again.")]
    CatalogUnavailable,
}

type LoginResult = Shared<BoxFuture<'static, Result<ProviderLoginOutcome, ProviderLoginError>>>;

struct LoginRun {
    cancellation: CancellationToken,
    result: LoginResult,
}

impl LoginRun {
    fn retains_custody(&self) -> bool {
        matches!(
            self.result.peek(),
            None | Some(Err(ProviderLoginError::CleanupUnconfirmed))
        )
    }
}

struct LoginOwner {
    cancellation: CancellationToken,
    catalog: crate::ProviderCatalogService,
    runs: Mutex<BTreeMap<&'static str, LoginRun>>,
}

impl Drop for LoginOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Clone)]
pub struct ProviderLoginService(Arc<LoginOwner>);

impl ProviderLoginService {
    #[must_use]
    pub fn new(cancellation: CancellationToken, catalog: crate::ProviderCatalogService) -> Self {
        Self(Arc::new(LoginOwner {
            cancellation,
            catalog,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }

    /// Starts or joins the provider's current login, without starting a model turn.
    ///
    /// # Errors
    /// Reports unsupported, missing, failed, cancelled and unconfirmed cleanup states.
    pub async fn login(
        &self,
        provider_id: &str,
    ) -> Result<ProviderLoginOutcome, ProviderLoginError> {
        let registration = provider_registration_by_id(provider_id)
            .filter(|registration| registration.login.is_some())
            .ok_or(ProviderLoginError::Unsupported)?;
        self.login_registered(registration).await
    }

    async fn login_registered(
        &self,
        registration: &'static ProviderRegistration,
    ) -> Result<ProviderLoginOutcome, ProviderLoginError> {
        let result = {
            let mut runs = self.0.runs.lock().await;
            if self.0.cancellation.is_cancelled() {
                return Err(ProviderLoginError::Cancelled);
            }
            if let Some(run) = runs
                .get(registration.id)
                .filter(|run| run.retains_custody())
            {
                run.result.clone()
            } else {
                let cancellation = self.0.cancellation.child_token();
                let operation_cancellation = cancellation.clone();
                let catalog = self.0.catalog.clone();
                let task = tokio::spawn(async move {
                    let outcome = run_login(registration, &operation_cancellation).await?;
                    if outcome == ProviderLoginOutcome::Authenticated {
                        let refreshed = catalog.refresh_provider(registration.id, true).await;
                        if !refreshed.is_ok_and(|catalog| {
                            catalog.status == "ready"
                                && catalog.providers.iter().any(|provider| {
                                    provider.id == registration.id
                                        && provider.discovery_status == "ready"
                                })
                        }) {
                            return Err(ProviderLoginError::CatalogUnavailable);
                        }
                    }
                    Ok(outcome)
                });
                let result = async move {
                    task.await
                        .map_err(|_| ProviderLoginError::CleanupUnconfirmed)?
                }
                .boxed()
                .shared();
                runs.insert(
                    registration.id,
                    LoginRun {
                        cancellation,
                        result: result.clone(),
                    },
                );
                result
            }
        };
        result.await
    }

    /// Cancels and joins an active login. False means no active login was cancelled.
    ///
    /// # Errors
    /// Does not claim cancellation when process cleanup fails.
    pub async fn cancel(&self, provider_id: &str) -> Result<bool, ProviderLoginError> {
        let result = {
            let runs = self.0.runs.lock().await;
            let Some(run) = runs.get(provider_id).filter(|run| run.retains_custody()) else {
                return Ok(false);
            };
            run.cancellation.cancel();
            run.result.clone()
        };
        match result.await {
            Err(ProviderLoginError::Cancelled) => Ok(true),
            Ok(_) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Cancels and joins every owned login before runtime shutdown completes.
    ///
    /// # Errors
    /// Reports unconfirmed process cleanup or task failure.
    pub async fn shutdown(&self) -> Result<(), ProviderLoginError> {
        self.0.cancellation.cancel();
        let results: Vec<_> = self
            .0
            .runs
            .lock()
            .await
            .values()
            .map(|run| run.result.clone())
            .collect();
        let results = join_all(results).await;
        if results.contains(&Err(ProviderLoginError::CleanupUnconfirmed)) {
            return Err(ProviderLoginError::CleanupUnconfirmed);
        }
        // Authentication outcomes were returned to callers; only custody failure blocks shutdown.
        Ok(())
    }
}

async fn run_login(
    registration: &ProviderRegistration,
    cancellation: &CancellationToken,
) -> Result<ProviderLoginOutcome, ProviderLoginError> {
    let spec = registration
        .login
        .as_ref()
        .ok_or(ProviderLoginError::Unsupported)?;
    let (executable, _) = provider_executable(registration.probe_executable, cancellation)
        .await
        .map_err(login_failure)?;
    if matches!(spec.flow, ProviderLoginFlow::InteractiveTerminal) {
        crate::terminal_login::launch(&executable, spec.arguments, cancellation).await?;
        return Ok(ProviderLoginOutcome::Started);
    }
    let environment = spec
        .environment
        .map_or_else(|| Ok(Vec::new()), |environment| environment())
        .map_err(|_| ProviderLoginError::Failed)?;
    // Login output may contain an authorization URL or account data. It is never published.
    probe_with_timeout(
        &executable,
        spec.arguments,
        Duration::from_mins(10),
        cancellation,
        &environment,
    )
    .await
    .map(|_| ProviderLoginOutcome::Authenticated)
    .map_err(login_failure)
}

pub(crate) fn login_failure(error: ProbeFailure) -> ProviderLoginError {
    match error {
        ProbeFailure::Missing => ProviderLoginError::Missing,
        ProbeFailure::Timeout => ProviderLoginError::Timeout,
        ProbeFailure::Cancelled => ProviderLoginError::Cancelled,
        ProbeFailure::CleanupUnconfirmed => ProviderLoginError::CleanupUnconfirmed,
        ProbeFailure::Authentication
        | ProbeFailure::Malformed
        | ProbeFailure::Failed
        | ProbeFailure::CatalogTooLarge => ProviderLoginError::Failed,
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn ready_discovery<'a>(
        mut provider: agentsassemble_domain::ProviderAvailability,
        _: &'a crate::ProviderCredentialStore,
        _: &'a CancellationToken,
    ) -> crate::registration::ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            provider.discovery_status = "ready".into();
            provider.available = true;
            provider.startable = true;
            provider
        })
    }

    fn ready_catalog() -> crate::ProviderCatalogService {
        crate::ProviderCatalogService::discovering_registrations(
            vec![&SUCCESS],
            &crate::ProviderCredentialStore::isolated_test_store(),
            false,
        )
    }

    static SUCCESS: ProviderRegistration = ProviderRegistration {
        discover: ready_discovery,
        probe_executable: "/usr/bin/true",
        login: Some(ProviderLoginSpec {
            flow: ProviderLoginFlow::BrowserOauth,
            arguments: &[],
            environment: None,
        }),
        ..crate::registration::CODEX_PROVIDER
    };
    static FAILURE: ProviderRegistration = ProviderRegistration {
        probe_executable: "/bin/sh",
        login: Some(ProviderLoginSpec {
            flow: ProviderLoginFlow::BrowserOauth,
            arguments: &["-c", "printf 'private-login-diagnostic' >&2; exit 1"],
            environment: None,
        }),
        ..crate::registration::CODEX_PROVIDER
    };

    #[tokio::test]
    async fn login_reports_native_exit_without_private_output_and_shutdown_rejects_new_work() {
        let service = ProviderLoginService::new(CancellationToken::new(), ready_catalog());
        let (first, retry) = tokio::join!(
            service.login_registered(&SUCCESS),
            service.login_registered(&SUCCESS)
        );
        assert_eq!(
            (first, retry),
            (
                Ok(ProviderLoginOutcome::Authenticated),
                Ok(ProviderLoginOutcome::Authenticated)
            )
        );
        assert_eq!(
            service.login_registered(&FAILURE).await,
            Err(ProviderLoginError::Failed)
        );
        assert_eq!(
            service.login_registered(&SUCCESS).await,
            Ok(ProviderLoginOutcome::Authenticated)
        );
        for provider in ["custom_api", "antigravity", "freebuff", "unknown"] {
            assert_eq!(
                service.login(provider).await,
                Err(ProviderLoginError::Unsupported)
            );
        }
        assert_eq!(service.cancel("codex").await, Ok(false));
        assert_eq!(service.shutdown().await, Ok(()));
        assert_eq!(
            service.login_registered(&SUCCESS).await,
            Err(ProviderLoginError::Cancelled)
        );
    }

    #[tokio::test]
    async fn dropped_login_request_keeps_catalog_completion_owned_and_reports_discovery_failure() {
        static ENTERED: tokio::sync::Notify = tokio::sync::Notify::const_new();
        static RELEASE: tokio::sync::Notify = tokio::sync::Notify::const_new();
        fn gated<'a>(
            provider: agentsassemble_domain::ProviderAvailability,
            credentials: &'a crate::ProviderCredentialStore,
            cancellation: &'a CancellationToken,
        ) -> crate::registration::ProviderDiscoveryFuture<'a> {
            Box::pin(async move {
                ENTERED.notify_one();
                RELEASE.notified().await;
                ready_discovery(provider, credentials, cancellation).await
            })
        }
        fn failed<'a>(
            provider: agentsassemble_domain::ProviderAvailability,
            _: &'a crate::ProviderCredentialStore,
            _: &'a CancellationToken,
        ) -> crate::registration::ProviderDiscoveryFuture<'a> {
            Box::pin(async move { crate::catalog::failed_provider(provider, ProbeFailure::Failed) })
        }
        static GATED: ProviderRegistration = ProviderRegistration {
            discover: gated,
            login: Some(ProviderLoginSpec {
                flow: ProviderLoginFlow::BrowserOauth,
                arguments: &[],
                environment: None,
            }),
            ..SUCCESS
        };
        static FAILED: ProviderRegistration = ProviderRegistration {
            discover: failed,
            login: Some(ProviderLoginSpec {
                flow: ProviderLoginFlow::BrowserOauth,
                arguments: &[],
                environment: None,
            }),
            ..SUCCESS
        };
        let catalog = crate::ProviderCatalogService::discovering_registrations(
            vec![&GATED],
            &crate::ProviderCredentialStore::isolated_test_store(),
            false,
        );
        let service = ProviderLoginService::new(CancellationToken::new(), catalog.clone());
        let requester = service.clone();
        let request = tokio::spawn(async move { requester.login_registered(&GATED).await });
        ENTERED.notified().await;
        request.abort();
        assert!(request.await.is_err());
        let joined = service.login_registered(&GATED);
        tokio::pin!(joined);
        assert!(futures_util::poll!(&mut joined).is_pending());
        RELEASE.notify_one();
        assert_eq!(joined.await, Ok(ProviderLoginOutcome::Authenticated));
        assert_eq!(catalog.snapshot().providers[0].discovery_status, "ready");
        assert_eq!(service.shutdown().await, Ok(()));
        assert!(catalog.shutdown().await.is_ok());

        let catalog = crate::ProviderCatalogService::discovering_registrations(
            vec![&FAILED],
            &crate::ProviderCredentialStore::isolated_test_store(),
            false,
        );
        let service = ProviderLoginService::new(CancellationToken::new(), catalog.clone());
        assert_eq!(
            service.login_registered(&FAILED).await,
            Err(ProviderLoginError::CatalogUnavailable)
        );
        assert_eq!(service.shutdown().await, Ok(()));
        assert!(catalog.shutdown().await.is_ok());
    }

    #[tokio::test]
    async fn observed_cleanup_failure_blocks_replacement_and_survives_cancel_and_shutdown() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static LAUNCHES: AtomicUsize = AtomicUsize::new(0);
        fn interrupted_launch() -> Result<Vec<(String, String)>, crate::runtime::DriverError> {
            LAUNCHES.fetch_add(1, Ordering::SeqCst);
            panic!("controlled login task loss before native authentication launch");
        }
        static UNCONFIRMED: ProviderRegistration = ProviderRegistration {
            probe_executable: "/usr/bin/true",
            login: Some(ProviderLoginSpec {
                flow: ProviderLoginFlow::BrowserOauth,
                arguments: &[],
                environment: Some(interrupted_launch),
            }),
            ..crate::registration::CODEX_PROVIDER
        };
        let service = ProviderLoginService::new(CancellationToken::new(), ready_catalog());
        let expected = Err(ProviderLoginError::CleanupUnconfirmed);
        assert_eq!(service.login_registered(&UNCONFIRMED).await, expected);
        assert_eq!(LAUNCHES.load(Ordering::SeqCst), 1);
        // The first failure is already observed, including Shared::peek returning Some.
        assert_eq!(service.login_registered(&UNCONFIRMED).await, expected);
        assert_eq!(service.login_registered(&SUCCESS).await, expected);
        assert_eq!(LAUNCHES.load(Ordering::SeqCst), 1);
        assert_eq!(
            service.cancel("codex").await,
            Err(ProviderLoginError::CleanupUnconfirmed)
        );
        for _ in 0..2 {
            assert_eq!(
                service.shutdown().await,
                Err(ProviderLoginError::CleanupUnconfirmed)
            );
            assert_eq!(
                service.cancel("codex").await,
                Err(ProviderLoginError::CleanupUnconfirmed)
            );
        }
        assert_eq!(
            service.login_registered(&SUCCESS).await,
            Err(ProviderLoginError::Cancelled)
        );
    }
}
