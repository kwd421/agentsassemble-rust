use crate::{
    catalog::{provider_executable, resolved_codex},
    registration::provider_registration_by_id,
};
use agentsassemble_domain::ProviderUpdate;
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared, join_all},
};
use std::{collections::BTreeMap, sync::Arc};
use tokio::{sync::Mutex, task::AbortHandle};
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ProviderUpdateError {
    #[error(
        "This installation does not support an in-app update check or native updater. Use its official instructions."
    )]
    Unsupported,
    #[error("The provider executable is unavailable.")]
    Missing,
    #[error("Another provider setup operation is in progress.")]
    Busy,
    #[error("The offered version changed or is no longer newer. Check versions again.")]
    OfferChanged,
    #[error("Provider version information is unavailable.")]
    Unavailable,
    #[error("Provider version information is invalid.")]
    InvalidResponse,
    #[error("Provider version operation was cancelled.")]
    Cancelled,
    #[error("Provider version process cleanup could not be confirmed.")]
    CleanupUnconfirmed,
    #[error("The update terminal may have opened. Check it before trying again.")]
    HandoffUnconfirmed,
}

impl From<crate::process::ProbeFailure> for ProviderUpdateError {
    fn from(error: crate::process::ProbeFailure) -> Self {
        use crate::process::ProbeFailure as E;
        match error {
            E::Missing => Self::Missing,
            E::Cancelled => Self::Cancelled,
            E::CleanupUnconfirmed => Self::CleanupUnconfirmed,
            E::Malformed | E::CatalogTooLarge => Self::InvalidResponse,
            E::Authentication | E::Timeout | E::Failed => Self::Unavailable,
        }
    }
}

type ResultFuture = Shared<BoxFuture<'static, Result<ProviderUpdate, ProviderUpdateError>>>;
struct UpdateRun {
    expected: Option<String>,
    task: AbortHandle,
    result: ResultFuture,
}
struct UpdateOwner {
    cancellation: CancellationToken,
    runs: Mutex<BTreeMap<&'static str, UpdateRun>>,
}
impl Drop for UpdateOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}
#[derive(Clone)]
pub struct ProviderUpdateService(Arc<UpdateOwner>);
impl ProviderUpdateService {
    #[must_use]
    pub fn new(cancellation: CancellationToken) -> Self {
        Self(Arc::new(UpdateOwner {
            cancellation,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }
    /// None only reads versions. Some requires a fresh matching offer before native handoff.
    /// # Errors
    /// Reports unsupported, busy, stale offer, read, cancellation and custody failures.
    pub async fn perform(
        &self,
        provider_id: &str,
        expected: Option<String>,
    ) -> Result<ProviderUpdate, ProviderUpdateError> {
        let registration = provider_registration_by_id(provider_id)
            .filter(|r| r.update.is_some())
            .ok_or(ProviderUpdateError::Unsupported)?;
        self.perform_registered(registration, expected).await
    }

    async fn perform_registered(
        &self,
        registration: &'static crate::registration::ProviderRegistration,
        expected: Option<String>,
    ) -> Result<ProviderUpdate, ProviderUpdateError> {
        if expected
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 64)
        {
            return Err(ProviderUpdateError::OfferChanged);
        }
        let result = {
            let mut runs = self.0.runs.lock().await;
            if self.0.cancellation.is_cancelled() {
                return Err(ProviderUpdateError::Cancelled);
            }
            if let Some(run) = runs.get(registration.id) {
                if !run.task.is_finished() {
                    if run.expected != expected {
                        return Err(ProviderUpdateError::Busy);
                    }
                    let result = run.result.clone();
                    drop(runs);
                    return result.await;
                }
                if run.result.clone().await == Err(ProviderUpdateError::CleanupUnconfirmed) {
                    return Err(ProviderUpdateError::CleanupUnconfirmed);
                }
            }
            let cancellation = self.0.cancellation.child_token();
            let requested = expected.clone();
            let task = tokio::spawn(async move {
                let spec = registration
                    .update
                    .ok_or(ProviderUpdateError::Unsupported)?;
                let (executable, _) = if spec.bundled_codex {
                    resolved_codex(&cancellation).await
                } else {
                    provider_executable(registration.probe_executable, &cancellation).await
                }
                .map_err(ProviderUpdateError::from)?;
                let mut observation = spec
                    .read(registration.id, &executable, &cancellation)
                    .await?;
                if let Some(expected) = requested {
                    if !observation.update_available || observation.latest_version != expected {
                        return Err(ProviderUpdateError::OfferChanged);
                    }
                    let arguments = spec
                        .update_arguments
                        .ok_or(ProviderUpdateError::Unsupported)?;
                    crate::terminal_login::launch(&executable, arguments, &cancellation)
                        .await
                        .map_err(|error| match error {
                            crate::ProviderLoginError::Cancelled => ProviderUpdateError::Cancelled,
                            crate::ProviderLoginError::CleanupUnconfirmed => {
                                ProviderUpdateError::CleanupUnconfirmed
                            }
                            crate::ProviderLoginError::Unsupported => {
                                ProviderUpdateError::Unsupported
                            }
                            crate::ProviderLoginError::Missing => ProviderUpdateError::Missing,
                            _ => ProviderUpdateError::HandoffUnconfirmed,
                        })?;
                    observation.handoff_started = true;
                }
                Ok(observation)
            });
            let completion = task.abort_handle();
            let result = async move {
                task.await
                    .map_err(|_| ProviderUpdateError::CleanupUnconfirmed)?
            }
            .boxed()
            .shared();
            runs.insert(
                registration.id,
                UpdateRun {
                    expected,
                    task: completion,
                    result: result.clone(),
                },
            );
            result
        };
        result.await
    }
    /// Cancels and joins checks and launcher helpers; handed-off terminals belong to the user.
    /// # Errors
    /// Reports unconfirmed process cleanup.
    pub async fn shutdown(&self) -> Result<(), ProviderUpdateError> {
        self.0.cancellation.cancel();
        let runs = std::mem::take(&mut *self.0.runs.lock().await);
        let results = join_all(runs.into_values().map(|run| run.result)).await;
        if results.contains(&Err(ProviderUpdateError::CleanupUnconfirmed)) {
            return Err(ProviderUpdateError::CleanupUnconfirmed);
        }
        Ok(())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test]
    async fn check_never_updates_and_lost_or_changed_offers_are_rechecked()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let executable = root.path().join("provider");
        let calls = root.path().join("calls");
        let calls_arg = shlex::try_quote(calls.to_str().ok_or("fixture path")?)?;
        let write_version = |latest: &str| -> std::io::Result<()> {
            std::fs::write(
                &executable,
                format!(
                    "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {calls_arg}\nprintf '%s' '{{\"currentVersion\":\"1.0.0\",\"latestVersion\":\"{latest}\",\"updateAvailable\":true,\"error\":null}}'\n"
                ),
            )
        };
        write_version("2.0.0")?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let registration = Box::leak(Box::new(crate::registration::ProviderRegistration {
            probe_executable: Box::leak(
                executable
                    .to_str()
                    .ok_or("fixture path")?
                    .to_owned()
                    .into_boxed_str(),
            ),
            login: None,
            ..crate::registration::GROK_PROVIDER
        }));
        let service = ProviderUpdateService::new(CancellationToken::new());
        let first = service.perform_registered(registration, None).await?;
        assert!(!first.handoff_started);
        assert!(first.update_available);
        write_version("3.0.0")?;
        assert_eq!(
            service
                .perform_registered(registration, Some(first.latest_version))
                .await,
            Err(ProviderUpdateError::OfferChanged)
        );
        assert_eq!(
            service
                .perform_registered(registration, None)
                .await?
                .latest_version,
            "3.0.0"
        );
        assert_eq!(
            std::fs::read_to_string(calls)?,
            "update --check --json\n".repeat(3)
        );
        service.shutdown().await?;
        assert_eq!(
            service.perform_registered(registration, None).await,
            Err(ProviderUpdateError::Cancelled)
        );
        Ok(())
    }
}
