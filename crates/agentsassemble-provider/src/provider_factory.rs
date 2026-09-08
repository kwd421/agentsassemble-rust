#[cfg(unix)]
use crate::driver::DriverError;
use std::path::{Path, PathBuf};

use agentsassemble_domain::DurableAgentSession;

#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    credentials::ProviderCredentialStore,
    driver::{DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    runtime_lease::HeldRuntimeLease,
};
#[cfg(unix)]
use std::sync::OnceLock;

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
    pub(crate) managed: bool,
    #[cfg(unix)]
    pub(crate) guardian: OnceLock<Result<GuardianLaunch, DriverError>>,
}

impl ProductionDriverFactory {
    pub(crate) fn local(credentials: ProviderCredentialStore) -> Self {
        Self {
            credentials,
            state_root: None,
            #[cfg(unix)]
            managed: true,
            #[cfg(unix)]
            guardian: OnceLock::new(),
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
            managed: false,
            guardian: OnceLock::from(
                GuardianLaunch::production(executable).map_err(|_| custody_binding_failed()),
            ),
        }
    }

    #[cfg(unix)]
    pub(crate) fn guardian(&self) -> Result<&GuardianLaunch, DriverError> {
        self.guardian
            .get_or_init(bind_current_guardian)
            .as_ref()
            .map_err(Clone::clone)
    }
}

#[cfg(unix)]
fn bind_current_guardian() -> Result<GuardianLaunch, DriverError> {
    #[cfg(test)]
    return GuardianLaunch::test_harness().map_err(|_| custody_binding_failed());
    #[cfg(not(test))]
    crate::guardian::reexecution_path()
        .map_err(|_| custody_reexecution_failed())
        .and_then(|executable| {
            GuardianLaunch::production(&executable).map_err(|_| custody_binding_failed())
        })
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
