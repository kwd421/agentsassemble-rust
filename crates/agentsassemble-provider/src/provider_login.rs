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

pub(crate) struct ProviderLoginSpec {
    pub(crate) arguments: &'static [&'static str],
    pub(crate) environment: Option<LoginEnvironment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderLoginError {
    #[error("This provider does not support local login.")]
    Unsupported,
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
}

type LoginResult = Shared<BoxFuture<'static, Result<(), ProviderLoginError>>>;

struct LoginRun {
    cancellation: CancellationToken,
    result: LoginResult,
}

struct LoginOwner {
    cancellation: CancellationToken,
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
    pub fn new(cancellation: CancellationToken) -> Self {
        Self(Arc::new(LoginOwner {
            cancellation,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }

    /// Starts or joins the provider's current login, without starting a model turn.
    ///
    /// # Errors
    /// Reports unsupported, missing, failed, cancelled and unconfirmed cleanup states.
    pub async fn login(&self, provider_id: &str) -> Result<(), ProviderLoginError> {
        let registration = provider_registration_by_id(provider_id)
            .filter(|registration| registration.login.is_some())
            .ok_or(ProviderLoginError::Unsupported)?;
        self.login_registered(registration).await
    }

    async fn login_registered(
        &self,
        registration: &'static ProviderRegistration,
    ) -> Result<(), ProviderLoginError> {
        let result = {
            let mut runs = self.0.runs.lock().await;
            if self.0.cancellation.is_cancelled() {
                return Err(ProviderLoginError::Cancelled);
            }
            if let Some(run) = runs
                .get(registration.id)
                .filter(|run| run.result.peek().is_none())
            {
                run.result.clone()
            } else {
                let cancellation = self.0.cancellation.child_token();
                let operation_cancellation = cancellation.clone();
                let task =
                    tokio::spawn(
                        async move { run_login(registration, &operation_cancellation).await },
                    );
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
            let Some(run) = runs
                .get(provider_id)
                .filter(|run| run.result.peek().is_none())
            else {
                return Ok(false);
            };
            run.cancellation.cancel();
            run.result.clone()
        };
        match result.await {
            Err(ProviderLoginError::Cancelled) => Ok(true),
            Ok(()) => Ok(false),
            Err(error) => Err(error),
        }
    }

    /// Cancels and joins every owned login before runtime shutdown completes.
    ///
    /// # Errors
    /// Reports unconfirmed process cleanup or task failure.
    pub async fn shutdown(&self) -> Result<(), ProviderLoginError> {
        self.0.cancellation.cancel();
        let runs = std::mem::take(&mut *self.0.runs.lock().await);
        let results = join_all(runs.into_values().map(|run| run.result)).await;
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
) -> Result<(), ProviderLoginError> {
    let spec = registration
        .login
        .as_ref()
        .ok_or(ProviderLoginError::Unsupported)?;
    let (executable, _) = provider_executable(registration.probe_executable, cancellation)
        .await
        .map_err(login_failure)?;
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
    .map(|_| ())
    .map_err(login_failure)
}

fn login_failure(error: ProbeFailure) -> ProviderLoginError {
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

    static SUCCESS: ProviderRegistration = ProviderRegistration {
        probe_executable: "/usr/bin/true",
        login: Some(ProviderLoginSpec {
            arguments: &[],
            environment: None,
        }),
        ..crate::registration::CODEX_PROVIDER
    };
    static FAILURE: ProviderRegistration = ProviderRegistration {
        probe_executable: "/bin/sh",
        login: Some(ProviderLoginSpec {
            arguments: &["-c", "printf 'private-login-diagnostic' >&2; exit 1"],
            environment: None,
        }),
        ..crate::registration::CODEX_PROVIDER
    };

    #[tokio::test]
    async fn login_reports_native_exit_without_private_output_and_shutdown_rejects_new_work() {
        let service = ProviderLoginService::new(CancellationToken::new());
        let (first, retry) = tokio::join!(
            service.login_registered(&SUCCESS),
            service.login_registered(&SUCCESS)
        );
        assert_eq!((first, retry), (Ok(()), Ok(())));
        assert_eq!(
            service.login_registered(&FAILURE).await,
            Err(ProviderLoginError::Failed)
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
}
