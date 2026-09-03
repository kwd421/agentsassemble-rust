use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::Arc;

use agentsassemble_domain::DurableAgentSession;

#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    credentials::ProviderCredentialStore,
    driver::{DriverError, DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    runtime_lease::HeldRuntimeLease,
};

pub(crate) trait DriverFactory: Send + Sync {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
        runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>>;
}

pub(crate) struct ProductionDriverFactory {
    pub(crate) credentials: ProviderCredentialStore,
    pub(crate) state_root: Option<PathBuf>,
    #[cfg(unix)]
    pub(crate) guardian: Result<Option<GuardianLaunch>, DriverError>,
    #[cfg(windows)]
    pub(crate) companion: Result<Arc<crate::filesystem::BoundExecutable>, DriverError>,
}

impl ProductionDriverFactory {
    pub(crate) fn local(credentials: ProviderCredentialStore) -> Self {
        #[cfg(all(unix, test))]
        let guardian = GuardianLaunch::test_harness()
            .map(Some)
            .map_err(|_| custody_binding_failed());
        #[cfg(all(unix, not(test), any(target_os = "linux", target_os = "android")))]
        let guardian = crate::guardian::reexecution_path()
            .map_err(|_| custody_reexecution_failed())
            .and_then(|executable| {
                GuardianLaunch::production(&executable)
                    .map(Some)
                    .map_err(|_| custody_binding_failed())
            });
        #[cfg(all(unix, not(test), not(any(target_os = "linux", target_os = "android"))))]
        let guardian =
            if std::env::var_os("AGENTSASSEMBLE_INTERNAL_SERVER_STAGED") == Some("v1".into()) {
                crate::guardian::reexecution_path()
                    .map_err(|_| custody_reexecution_failed())
                    .and_then(|executable| {
                        GuardianLaunch::production(&executable)
                            .map(Some)
                            .map_err(|_| custody_binding_failed())
                    })
            } else {
                Ok(None)
            };
        Self {
            credentials,
            state_root: None,
            #[cfg(unix)]
            guardian,
            #[cfg(windows)]
            companion: crate::filesystem::bind_current_helper_executable()
                .map(Arc::new)
                .map_err(|_| companion_binding_failed()),
        }
    }

    pub(crate) fn at_state_root(credentials: ProviderCredentialStore, state_root: &Path) -> Self {
        let mut factory = Self::local(credentials);
        factory.state_root = Some(state_root.to_path_buf());
        factory
    }

    #[cfg(unix)]
    pub(crate) fn with_guardian(executable: &Path) -> Self {
        Self {
            credentials: ProviderCredentialStore::production(),
            state_root: None,
            guardian: GuardianLaunch::production(executable)
                .map(Some)
                .map_err(|_| custody_binding_failed()),
        }
    }

    #[cfg(unix)]
    pub(crate) fn guardian(&self) -> Result<&GuardianLaunch, DriverError> {
        self.guardian
            .as_ref()
            .map_err(|error| *error)?
            .as_ref()
            .ok_or_else(custody_unavailable)
    }

    #[cfg(windows)]
    pub(crate) fn companion(&self) -> Result<&crate::filesystem::BoundExecutable, DriverError> {
        self.companion.as_deref().map_err(|error| *error)
    }
}

#[cfg(unix)]
const fn custody_unavailable() -> DriverError {
    DriverError::new(
        "provider_custody_unavailable",
        "The provider process custody helper is unavailable.",
    )
}

#[cfg(all(unix, not(test)))]
const fn custody_reexecution_failed() -> DriverError {
    DriverError::new(
        "provider_custody_reexecution_failed",
        "The provider process custody executable could not be resolved.",
    )
}

#[cfg(unix)]
const fn custody_binding_failed() -> DriverError {
    DriverError::new(
        "provider_custody_binding_failed",
        "The provider process custody executable could not be bound.",
    )
}

#[cfg(windows)]
const fn companion_binding_failed() -> DriverError {
    DriverError::new(
        "provider_companion_binding_failed",
        "The private provider companion executable could not be bound.",
    )
}
