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
    #[error("The provider updater failed; installation completion is unconfirmed.")]
    InstallationUnconfirmed,
    #[error("The provider was updated, but its model catalog could not be refreshed.")]
    CatalogUnavailable,
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
    catalog: crate::ProviderCatalogService,
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
    pub fn new(cancellation: CancellationToken, catalog: crate::ProviderCatalogService) -> Self {
        Self(Arc::new(UpdateOwner {
            cancellation,
            catalog,
            runs: Mutex::new(BTreeMap::new()),
        }))
    }
    /// None reads versions or joins an active update. Some requires a matching offer.
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
                    if expected.is_some() && run.expected != expected {
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
            let catalog = self.0.catalog.clone();
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
                let observation = spec
                    .read(registration.id, &executable, &cancellation)
                    .await?;
                if let Some(expected) = requested {
                    if !observation.update_available || observation.latest_version != expected {
                        return Err(ProviderUpdateError::OfferChanged);
                    }
                    let installed =
                        install(registration, &executable, &expected, &cancellation).await?;
                    // This task owns completion even if the requesting HTTP handler disappears.
                    let refreshed = catalog.refresh_provider(registration.id, true).await;
                    if !refreshed.is_ok_and(|catalog| {
                        catalog.status == "ready"
                            && catalog.providers.iter().any(|provider| {
                                provider.id == registration.id
                                    && provider.discovery_status == "ready"
                                    && provider.startable
                            })
                    }) {
                        return Err(ProviderUpdateError::CatalogUnavailable);
                    }
                    return Ok(installed);
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
    /// Cancels and joins every owned check/updater, retaining unconfirmed custody.
    /// # Errors
    /// Reports unconfirmed process cleanup.
    pub async fn shutdown(&self) -> Result<(), ProviderUpdateError> {
        self.0.cancellation.cancel();
        let results = self
            .0
            .runs
            .lock()
            .await
            .values()
            .map(|run| run.result.clone())
            .collect::<Vec<_>>();
        let results = join_all(results).await;
        if results.contains(&Err(ProviderUpdateError::CleanupUnconfirmed)) {
            return Err(ProviderUpdateError::CleanupUnconfirmed);
        }
        Ok(())
    }
}

async fn install(
    registration: &crate::registration::ProviderRegistration,
    executable: &str,
    expected: &str,
    cancellation: &CancellationToken,
) -> Result<ProviderUpdate, ProviderUpdateError> {
    let spec = registration
        .update
        .ok_or(ProviderUpdateError::Unsupported)?;
    let command = spec
        .updater
        .command(executable, expected, cancellation)
        .await?;
    let arguments = command
        .arguments
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>();
    crate::process::probe_with_timeout(
        &command.executable,
        &arguments,
        std::time::Duration::from_mins(10),
        cancellation,
        &[],
    )
    .await
    .map_err(|error| match error {
        crate::process::ProbeFailure::Cancelled => ProviderUpdateError::Cancelled,
        crate::process::ProbeFailure::CleanupUnconfirmed => ProviderUpdateError::CleanupUnconfirmed,
        _ => ProviderUpdateError::InstallationUnconfirmed,
    })?;
    let (installed, _) = if spec.bundled_codex {
        resolved_codex(cancellation).await
    } else {
        provider_executable(registration.probe_executable, cancellation).await
    }
    .map_err(ProviderUpdateError::from)?;
    let mut observation = spec.read(registration.id, &installed, cancellation).await?;
    if !spec.confirms_installation(&observation.current_version, expected)? {
        return Err(ProviderUpdateError::InstallationUnconfirmed);
    }
    observation.completed = true;
    Ok(observation)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    static CATALOG_FAILS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static CATALOG_READS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn refreshed_catalog<'a>(
        provider: agentsassemble_domain::ProviderAvailability,
        _: &'a crate::ProviderCredentialStore,
        _: &'a CancellationToken,
    ) -> crate::registration::ProviderDiscoveryFuture<'a> {
        Box::pin(async move {
            CATALOG_READS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if CATALOG_FAILS.load(std::sync::atomic::Ordering::SeqCst) {
                return crate::catalog::failed_provider(
                    provider,
                    crate::process::ProbeFailure::Failed,
                );
            }
            crate::catalog::ready_provider(
                provider,
                "updated-model".into(),
                vec![crate::catalog::control(
                    "model",
                    "Model",
                    "combobox",
                    vec![crate::catalog::option("updated-model", "Updated")],
                    "updated-model",
                )],
            )
        })
    }

    fn update_fixture() -> Result<
        (
            tempfile::TempDir,
            &'static crate::registration::ProviderRegistration,
        ),
        Box<dyn std::error::Error>,
    > {
        let root = tempfile::tempdir()?;
        let executable = root.path().join("grok-fixture");
        let root_literal = serde_json::to_string(root.path().to_str().ok_or("fixture path")?)?;
        std::fs::write(root.path().join("version"), "1.0.0")?;
        std::fs::write(root.path().join("mode"), "gated")?;
        std::fs::write(
            &executable,
            format!(
                r"#!/usr/bin/env python3
import sys,json,socket
from pathlib import Path
root=Path({root_literal})
with (root/'calls').open('a') as out: out.write(' '.join(sys.argv[1:])+'\n')
if '--check' in sys.argv:
    current=(root/'version').read_text()
    print(json.dumps(dict(currentVersion=current,latestVersion='2.0.0',updateAvailable=current=='1.0.0',error=None)))
else:
    mode=(root/'mode').read_text()
    if mode=='failure': sys.exit(1)
    if mode=='unchanged': sys.exit(0)
    with socket.socket(socket.AF_UNIX) as channel:
        channel.connect(str(root/'ready'))
        channel.sendall(b'1')
        channel.recv(1)
    (root/'version').write_text(sys.argv[-1])
"
            ),
        )?;
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        let registration: &'static crate::registration::ProviderRegistration =
            Box::leak(Box::new(crate::registration::ProviderRegistration {
                probe_executable: Box::leak(
                    executable
                        .to_str()
                        .ok_or("fixture path")?
                        .to_owned()
                        .into_boxed_str(),
                ),
                discover: refreshed_catalog,
                login: None,
                ..crate::registration::GROK_PROVIDER
            }));
        Ok((root, registration))
    }

    #[tokio::test]
    async fn update_outlives_its_request_and_check_joins_confirmed_installation_and_catalog()
    -> Result<(), Box<dyn std::error::Error>> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (root, registration) = update_fixture()?;
        let catalog = crate::ProviderCatalogService::discovering_registrations(
            vec![registration],
            &crate::ProviderCredentialStore::isolated_test_store(),
            false,
        );
        let service = ProviderUpdateService::new(CancellationToken::new(), catalog.clone());
        let listener = tokio::net::UnixListener::bind(root.path().join("ready"))?;
        let request_service = service.clone();
        let request = tokio::spawn(async move {
            request_service
                .perform_registered(registration, Some("2.0.0".into()))
                .await
        });
        let (mut gate, _) =
            tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept()).await??;
        gate.read_u8().await?;
        request.abort();
        assert!(request.await.is_err());
        assert_eq!(
            service
                .perform_registered(registration, Some("3.0.0".into()))
                .await,
            Err(ProviderUpdateError::Busy)
        );
        let joined = service.perform_registered(registration, None);
        tokio::pin!(joined);
        assert!(futures_util::poll!(&mut joined).is_pending());
        gate.write_all(b"1").await?;
        let result = joined.await?;
        assert!(result.completed);
        assert_eq!(result.current_version, "2.0.0");
        assert!(!result.update_available);
        assert_eq!(CATALOG_READS.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(
            std::fs::read_to_string(root.path().join("calls"))?,
            "update --check --json\nupdate --version 2.0.0\nupdate --check --json\n"
        );
        for mode in ["failure", "unchanged"] {
            std::fs::write(root.path().join("mode"), mode)?;
            std::fs::write(root.path().join("version"), "1.0.0")?;
            assert_eq!(
                service
                    .perform_registered(registration, Some("2.0.0".into()))
                    .await,
                Err(ProviderUpdateError::InstallationUnconfirmed)
            );
            assert_eq!(CATALOG_READS.load(std::sync::atomic::Ordering::SeqCst), 1);
        }
        // Successful installation is separate from a failed selected catalog.
        std::fs::write(root.path().join("mode"), "gated")?;
        CATALOG_FAILS.store(true, std::sync::atomic::Ordering::SeqCst);
        let update = service.perform_registered(registration, Some("2.0.0".into()));
        tokio::pin!(update);
        let release = async {
            let (mut gate, _) =
                tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
                    .await??;
            gate.read_u8().await?;
            gate.write_all(b"1").await
        };
        let (result, released) = tokio::join!(update, release);
        released?;
        assert_eq!(result, Err(ProviderUpdateError::CatalogUnavailable));
        assert_eq!(
            std::fs::read_to_string(root.path().join("version"))?,
            "2.0.0"
        );
        assert!(!catalog.snapshot().providers[0].startable);
        let calls_after_install = std::fs::read_to_string(root.path().join("calls"))?;
        CATALOG_FAILS.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(
            catalog
                .refresh_provider(registration.id, true)
                .await?
                .providers[0]
                .startable
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("calls"))?,
            calls_after_install
        );
        service.shutdown().await?;
        catalog.shutdown().await?;
        Ok(())
    }

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
        let service = ProviderUpdateService::new(
            CancellationToken::new(),
            crate::ProviderCatalogService::fixed(agentsassemble_domain::ProviderCatalog::default()),
        );
        let first = service.perform_registered(registration, None).await?;
        assert!(!first.completed);
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
