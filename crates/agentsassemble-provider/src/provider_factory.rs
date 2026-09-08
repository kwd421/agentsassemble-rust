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
    pub(crate) managed: bool,
    #[cfg(unix)]
    pub(crate) guardian: OnceLock<Result<GuardianLaunch, DriverError>>,
    #[cfg(windows)]
    worker: OnceLock<Result<crate::filesystem::BoundExecutable, DriverError>>,
}

impl ProductionDriverFactory {
    pub(crate) fn local(credentials: ProviderCredentialStore) -> Self {
        Self {
            credentials,
            state_root: None,
            managed: true,
            #[cfg(unix)]
            guardian: OnceLock::new(),
            #[cfg(windows)]
            worker: OnceLock::new(),
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

    #[cfg(windows)]
    pub(crate) fn with_worker(executable: &Path) -> Self {
        let mut factory = Self::local(ProviderCredentialStore::production());
        factory.worker = OnceLock::from(
            crate::filesystem::bind_helper_executable_sync(executable)
                .map_err(|_| custody_binding_failed()),
        );
        factory
    }

    #[cfg(windows)]
    pub(crate) fn worker(&self) -> Result<&crate::filesystem::BoundExecutable, DriverError> {
        self.worker
            .get_or_init(|| {
                let executable =
                    std::env::current_exe().map_err(|_| custody_reexecution_failed())?;
                crate::filesystem::bind_helper_executable_sync(&executable)
                    .map_err(|_| custody_binding_failed())
            })
            .as_ref()
            .map_err(Clone::clone)
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

#[cfg(any(windows, all(unix, not(test))))]
const fn custody_reexecution_failed() -> DriverError {
    DriverError::new(
        "provider_custody_reexecution_failed",
        "The provider process custody executable could not be resolved.",
    )
}

const fn custody_binding_failed() -> DriverError {
    DriverError::new(
        "provider_custody_binding_failed",
        "The provider process custody executable could not be bound.",
    )
}
