#[cfg(unix)]
use std::path::Path;
use std::{future::Future, pin::Pin, sync::Arc};

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability, ProviderCatalog};
use chrono::Utc;
use serde_json::Value;
use tokio::{
    sync::{Mutex, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

#[cfg(any(unix, windows))]
use crate::antigravity::AntigravityDriver;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    catalog::{
        catalog_revision, discover_antigravity, discover_codex, discover_deepseek,
        discover_opencode,
    },
    codex::CodexDriver,
    credentials::{ProviderCredentialStore, deepseek_credential_error},
    deepseek::DeepSeekDriver,
    driver::{DriverError, DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    opencode::OpenCodeDriver,
    runtime_lease::HeldRuntimeLease,
    selection::{ProviderSelection, ProviderSelectionError},
};

const MAX_PUBLIC_CATALOG_BYTES: usize = 48 * 1024;

pub(crate) type ProviderDiscoveryFuture<'a> =
    Pin<Box<dyn Future<Output = ProviderAvailability> + Send + 'a>>;
type ProviderDiscovery =
    for<'a> fn(ProviderAvailability, &'a CancellationToken) -> ProviderDiscoveryFuture<'a>;
type ProviderLaunch =
    for<'a> fn(
        &'a ProductionDriverFactory,
        &'a DurableAgentSession,
        &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>>;

pub(crate) struct ProviderRegistration {
    pub(crate) id: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) provider_kind: &'static str,
    pub(crate) runtime_kind: &'static str,
    pub(crate) transport: &'static str,
    pub(crate) catalog_group: &'static str,
    pub(crate) workspace_required: bool,
    pub(crate) connection_kind: &'static str,
    pub(crate) executable_required: bool,
    pub(crate) probe_executable: &'static str,
    pub(crate) login_label: &'static str,
    pub(crate) login_flow: &'static str,
    discover: ProviderDiscovery,
    launch: ProviderLaunch,
}

pub(crate) static CODEX_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "codex",
    display_name: "Codex",
    provider_kind: "codex_live_session",
    runtime_kind: "live_cli",
    transport: "stdio_jsonl",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "codex",
    login_label: "로그인",
    login_flow: "browser_oauth",
    discover: discover_codex_registered,
    launch: launch_codex,
};

pub(crate) static ANTIGRAVITY_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "antigravity",
    display_name: "Antigravity",
    provider_kind: "antigravity_live_session",
    runtime_kind: "live_cli",
    transport: if cfg!(windows) { "conpty" } else { "pty" },
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "agy",
    login_label: "로그인",
    login_flow: "interactive_terminal",
    discover: discover_antigravity_registered,
    launch: launch_antigravity,
};

pub(crate) static OPENCODE_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "opencode",
    display_name: "OpenCode",
    provider_kind: "opencode_server",
    runtime_kind: "opencode",
    transport: "http",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "opencode",
    login_label: "로그인",
    login_flow: "interactive_terminal",
    discover: discover_opencode_registered,
    launch: launch_opencode,
};

pub(crate) static DEEPSEEK_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "deepseek",
    display_name: "DeepSeek",
    provider_kind: "deepseek_api",
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    login_label: "API 키",
    login_flow: "api_key",
    discover: discover_deepseek_registered,
    launch: launch_deepseek,
};

static PROVIDER_REGISTRATIONS: [&ProviderRegistration; 4] = [
    &CODEX_PROVIDER,
    &ANTIGRAVITY_PROVIDER,
    &OPENCODE_PROVIDER,
    &DEEPSEEK_PROVIDER,
];

pub(crate) fn provider_registrations() -> &'static [&'static ProviderRegistration] {
    &PROVIDER_REGISTRATIONS
}

pub(crate) fn provider_registration_by_id(id: &str) -> Option<&'static ProviderRegistration> {
    PROVIDER_REGISTRATIONS
        .iter()
        .copied()
        .find(|registration| registration.id == id)
}

pub(crate) fn provider_registration_by_profile(
    provider_kind: &str,
    runtime_kind: &str,
    transport: &str,
) -> Option<&'static ProviderRegistration> {
    PROVIDER_REGISTRATIONS.iter().copied().find(|registration| {
        registration.provider_kind == provider_kind
            && registration.runtime_kind == runtime_kind
            && registration.transport == transport
    })
}

pub(crate) fn discover_provider<'a>(
    registration: &'static ProviderRegistration,
    cancellation: &'a CancellationToken,
) -> ProviderDiscoveryFuture<'a> {
    (registration.discover)(loading_provider(registration), cancellation)
}

fn discover_codex_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_codex(provider, cancellation))
}

fn discover_antigravity_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_antigravity(provider, cancellation))
}

fn discover_opencode_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_opencode(provider, cancellation))
}

fn discover_deepseek_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_deepseek(provider, cancellation))
}

#[derive(Clone)]
pub struct ProviderCatalogService {
    _sender: watch::Sender<ProviderCatalog>,
    receiver: watch::Receiver<ProviderCatalog>,
    owner: Arc<CatalogOwner>,
}

struct CatalogOwner {
    cancellation: CancellationToken,
    task: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for CatalogOwner {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

impl ProviderCatalogService {
    #[must_use]
    pub fn discovering() -> Self {
        let initial = loading_catalog();
        let (sender, receiver) = watch::channel(initial);
        let refresh_sender = sender.clone();
        let cancellation = CancellationToken::new();
        let discovery_cancellation = cancellation.clone();
        let task = tokio::spawn(async move {
            let catalog = discover_catalog(&discovery_cancellation).await;
            if !discovery_cancellation.is_cancelled() {
                let _ = refresh_sender.send(catalog);
            }
        });
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation,
                task: Mutex::new(Some(task)),
            }),
        }
    }

    #[must_use]
    pub fn fixed(catalog: ProviderCatalog) -> Self {
        let (sender, receiver) = watch::channel(bound_catalog(catalog));
        Self {
            _sender: sender,
            receiver,
            owner: Arc::new(CatalogOwner {
                cancellation: CancellationToken::new(),
                task: Mutex::new(None),
            }),
        }
    }

    #[must_use]
    pub fn snapshot(&self) -> ProviderCatalog {
        self.receiver.borrow().clone()
    }

    #[must_use]
    pub fn subscribe(&self) -> watch::Receiver<ProviderCatalog> {
        self.receiver.clone()
    }

    /// Cancels provider discovery and waits for its task to exit.
    ///
    /// # Errors
    ///
    /// Returns the discovery task's join error instead of hiding a panic or cancellation.
    pub async fn shutdown(&self) -> Result<(), tokio::task::JoinError> {
        self.owner.cancellation.cancel();
        if let Some(task) = self.owner.task.lock().await.take() {
            task.await?;
        }
        Ok(())
    }

    /// Validates a raw `agent.create` request against one exact catalog revision.
    ///
    /// # Errors
    ///
    /// Returns a stable fail-closed selection error.
    pub async fn validate_creation(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        payload: &Value,
    ) -> Result<ProviderSelection, ProviderSelectionError> {
        ProviderSelection::from_catalog(
            room_id,
            principal_id,
            request_id,
            payload,
            &self.snapshot(),
        )
        .await
    }
}

async fn discover_catalog(cancellation: &CancellationToken) -> ProviderCatalog {
    let providers = futures_util::future::join_all(
        provider_registrations()
            .iter()
            .map(|registration| discover_provider(registration, cancellation)),
    )
    .await;
    let (status, catalog_revision) = match catalog_revision(&providers) {
        Ok(revision) => ("ready".to_owned(), revision),
        Err(_) => ("failed".to_owned(), String::new()),
    };
    bound_catalog(ProviderCatalog {
        status,
        catalog_revision,
        discovered_at: Utc::now().to_rfc3339(),
        providers,
    })
}

fn bound_catalog(catalog: ProviderCatalog) -> ProviderCatalog {
    if serde_json::to_vec(&catalog).map_or(true, |encoded| encoded.len() > MAX_PUBLIC_CATALOG_BYTES)
    {
        return ProviderCatalog {
            status: "failed".to_owned(),
            catalog_revision: String::new(),
            discovered_at: catalog.discovered_at,
            providers: Vec::new(),
        };
    }
    catalog
}

fn loading_catalog() -> ProviderCatalog {
    ProviderCatalog {
        status: "loading".to_owned(),
        catalog_revision: String::new(),
        discovered_at: String::new(),
        providers: provider_registrations()
            .iter()
            .map(|registration| loading_provider(registration))
            .collect(),
    }
}

pub(crate) fn loading_provider(registration: &ProviderRegistration) -> ProviderAvailability {
    ProviderAvailability {
        id: registration.id.to_owned(),
        display_name: registration.display_name.to_owned(),
        provider_kind: registration.provider_kind.to_owned(),
        runtime_kind: registration.runtime_kind.to_owned(),
        catalog_group: registration.catalog_group.to_owned(),
        workspace_required: registration.workspace_required,
        connection_kind: registration.connection_kind.to_owned(),
        executable: registration.probe_executable.to_owned(),
        executable_identity: String::new(),
        default_model: String::new(),
        interactive: true,
        startable: false,
        available: false,
        discovery_status: "loading".to_owned(),
        catalog_source: "discovered".to_owned(),
        discovery_error_code: String::new(),
        discovery_error: String::new(),
        login_available: true,
        login_label: registration.login_label.to_owned(),
        login_flow: registration.login_flow.to_owned(),
        controls: Vec::new(),
    }
}

pub(crate) trait DriverFactory: Send + Sync {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
        runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>>;
}

pub(crate) struct ProductionDriverFactory {
    pub(crate) credentials: ProviderCredentialStore,
    #[cfg(unix)]
    pub(crate) guardian: Option<GuardianLaunch>,
    #[cfg(windows)]
    pub(crate) companion: Option<Arc<crate::filesystem::BoundExecutable>>,
}

impl ProductionDriverFactory {
    pub(crate) fn local(credentials: ProviderCredentialStore) -> Self {
        #[cfg(all(unix, test))]
        let guardian = GuardianLaunch::test_harness().ok();
        #[cfg(all(unix, not(test), any(target_os = "linux", target_os = "android")))]
        let guardian = crate::guardian::reexecution_path()
            .ok()
            .and_then(|executable| GuardianLaunch::production(&executable).ok());
        #[cfg(all(unix, not(test), not(any(target_os = "linux", target_os = "android"))))]
        let guardian = (std::env::var_os("AGENTSASSEMBLE_INTERNAL_SERVER_STAGED")
            == Some("v1".into()))
        .then(crate::guardian::reexecution_path)
        .and_then(Result::ok)
        .and_then(|executable| GuardianLaunch::production(&executable).ok());
        Self {
            credentials,
            #[cfg(unix)]
            guardian,
            #[cfg(windows)]
            companion: crate::filesystem::bind_current_helper_executable()
                .ok()
                .map(Arc::new),
        }
    }

    #[cfg(unix)]
    pub(crate) fn with_guardian(executable: &Path) -> Self {
        Self {
            credentials: ProviderCredentialStore::production(),
            guardian: GuardianLaunch::production(executable).ok(),
        }
    }
}

impl DriverFactory for ProductionDriverFactory {
    fn launch<'a>(
        &'a self,
        session: &'a DurableAgentSession,
        runtime_lease: &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
        let Some(registration) = provider_registration_by_profile(
            &session.public.provider_kind,
            &session.public.runtime_kind,
            &session.public.transport,
        ) else {
            return Box::pin(async {
                Err(DriverError::new(
                    "invalid_runtime_profile",
                    "The stored provider runtime profile is unsupported.",
                )
                .into())
            });
        };
        (registration.launch)(self, session, runtime_lease)
    }
}

fn launch_codex<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    #[cfg(not(unix))]
    let _ = (factory, runtime_lease);
    Box::pin(async move {
        #[cfg(unix)]
        let driver = CodexDriver::spawn(
            session,
            runtime_lease,
            factory.guardian.as_ref().ok_or_else(custody_unavailable)?,
        )
        .await?;
        #[cfg(not(unix))]
        let driver = CodexDriver::spawn(session).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_antigravity<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    #[cfg(not(any(unix, windows)))]
    let _ = (factory, session, runtime_lease);
    Box::pin(async move {
        #[cfg(unix)]
        {
            let driver = AntigravityDriver::spawn(
                session,
                runtime_lease,
                factory.guardian.as_ref().ok_or_else(custody_unavailable)?,
            )
            .await?;
            Ok(Box::new(driver) as Box<dyn ProviderDriver>)
        }
        #[cfg(windows)]
        {
            let driver = AntigravityDriver::spawn(
                session,
                factory.companion.as_deref().ok_or_else(|| {
                    DriverError::new(
                        "provider_custody_unavailable",
                        "The private provider companion is unavailable.",
                    )
                })?,
            )
            .await?;
            Ok(Box::new(driver) as Box<dyn ProviderDriver>)
        }
        #[cfg(not(any(unix, windows)))]
        Err(DriverError::new(
            "provider_runtime_unsupported",
            "Terminal provider sessions are unsupported on this platform.",
        )
        .into())
    })
}

fn launch_opencode<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    #[cfg(not(unix))]
    let _ = (factory, runtime_lease);
    Box::pin(async move {
        #[cfg(unix)]
        let driver = OpenCodeDriver::spawn(
            session,
            runtime_lease,
            factory.guardian.as_ref().ok_or_else(custody_unavailable)?,
        )
        .await?;
        #[cfg(not(unix))]
        let driver = OpenCodeDriver::spawn(session).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_deepseek<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .deepseek_secret()
            .await
            .map_err(|error| DriverLaunchError::safe(deepseek_credential_error(error)))?;
        let driver = DeepSeekDriver::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

#[cfg(unix)]
const fn custody_unavailable() -> DriverError {
    DriverError::new(
        "provider_custody_unavailable",
        "The provider process custody helper is unavailable.",
    )
}
