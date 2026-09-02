#[cfg(unix)]
use std::path::Path;
#[cfg(windows)]
use std::sync::Arc;
use std::{future::Future, pin::Pin};

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability};
use tokio_util::sync::CancellationToken;

#[cfg(any(unix, windows))]
use crate::antigravity::AntigravityDriver;
#[cfg(unix)]
use crate::guardian::GuardianLaunch;
use crate::{
    ProviderCredentialId,
    catalog::{
        discover_antigravity, discover_cerebras, discover_codex, discover_deepseek,
        discover_llm_gateway, discover_opencode, discover_openrouter, discover_tokenrouter,
        discover_vercel,
    },
    cerebras,
    codex::CodexDriver,
    credentials::ProviderCredentialStore,
    deepseek,
    driver::{DriverError, DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    llm_gateway,
    opencode::OpenCodeDriver,
    openrouter,
    runtime_lease::HeldRuntimeLease,
    tokenrouter, vercel,
};

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
    pub(crate) credential_available: bool,
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
    credential_available: false,
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
    credential_available: false,
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
    credential_available: false,
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
    credential_available: true,
    discover: discover_deepseek_registered,
    launch: launch_deepseek,
};

pub(crate) static CEREBRAS_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "cerebras",
    display_name: "Cerebras",
    provider_kind: "cerebras_api",
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    discover: discover_cerebras_registered,
    launch: launch_cerebras,
};

pub(crate) static OPENROUTER_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "openrouter",
    display_name: "OpenRouter",
    provider_kind: "openrouter_api",
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    discover: discover_openrouter_registered,
    launch: launch_openrouter,
};

pub(crate) static VERCEL_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "vercel",
    display_name: "Vercel AI Gateway",
    provider_kind: "vercel_ai_gateway",
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    discover: discover_vercel_registered,
    launch: launch_vercel,
};

pub(crate) static LLM_GATEWAY_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "llmgateway",
    display_name: llm_gateway::DISPLAY_NAME,
    provider_kind: llm_gateway::PROVIDER_KIND,
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    discover: discover_llm_gateway_registered,
    launch: launch_llm_gateway,
};

pub(crate) static TOKENROUTER_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "tokenrouter",
    display_name: tokenrouter::DISPLAY_NAME,
    provider_kind: tokenrouter::PROVIDER_KIND,
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    discover: discover_tokenrouter_registered,
    launch: launch_tokenrouter,
};

static PROVIDER_REGISTRATIONS: [&ProviderRegistration; 9] = [
    &CODEX_PROVIDER,
    &ANTIGRAVITY_PROVIDER,
    &OPENCODE_PROVIDER,
    &DEEPSEEK_PROVIDER,
    &CEREBRAS_PROVIDER,
    &OPENROUTER_PROVIDER,
    &VERCEL_PROVIDER,
    &LLM_GATEWAY_PROVIDER,
    &TOKENROUTER_PROVIDER,
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

fn discover_cerebras_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_cerebras(provider, cancellation))
}

fn discover_openrouter_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_openrouter(provider, cancellation))
}

fn discover_vercel_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_vercel(provider, cancellation))
}

fn discover_llm_gateway_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_llm_gateway(provider, cancellation))
}

fn discover_tokenrouter_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_tokenrouter(provider, cancellation))
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
        credential_available: registration.credential_available,
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
            .secret(ProviderCredentialId::DeepSeek)
            .await
            .map_err(|error| DriverLaunchError::safe(deepseek::credential_error(error)))?;
        let driver = deepseek::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_cerebras<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::Cerebras)
            .await
            .map_err(|error| DriverLaunchError::safe(cerebras::credential_error(error)))?;
        let driver = cerebras::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_openrouter<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::OpenRouter)
            .await
            .map_err(|error| DriverLaunchError::safe(openrouter::credential_error(error)))?;
        let driver = openrouter::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_vercel<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::Vercel)
            .await
            .map_err(|error| DriverLaunchError::safe(vercel::credential_error(error)))?;
        let driver = vercel::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_llm_gateway<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::LlmGateway)
            .await
            .map_err(|error| DriverLaunchError::safe(llm_gateway::credential_error(error)))?;
        let driver = llm_gateway::launch(factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_tokenrouter<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::TokenRouter)
            .await
            .map_err(|error| DriverLaunchError::safe(tokenrouter::credential_error(error)))?;
        let driver = tokenrouter::launch(factory.credentials.clone()).await?;
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
