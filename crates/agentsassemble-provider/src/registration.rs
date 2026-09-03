use std::{future::Future, pin::Pin};

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability};
use tokio_util::sync::CancellationToken;

#[cfg(any(unix, windows))]
use crate::antigravity::AntigravityDriver;
#[cfg(any(unix, windows))]
use crate::claude::ClaudeAgentSdkDriver;
use crate::{
    ProviderCredentialId,
    catalog::{
        discover_antigravity, discover_cerebras, discover_codex, discover_custom_api,
        discover_deepseek, discover_llm_gateway, discover_opencode, discover_openrouter,
        discover_tokenrouter, discover_vercel,
    },
    cerebras,
    codex::CodexDriver,
    cursor,
    cursor_acp::CursorAcpDriver,
    custom_api, deepseek,
    driver::{DriverError, DriverFuture, ProviderDriver},
    freebuff, grok,
    grok_acp::GrokAcpDriver,
    launch_error::DriverLaunchError,
    llm_gateway, lm_studio, ollama,
    opencode::OpenCodeDriver,
    openrouter,
    provider_factory::{DriverFactory, ProductionDriverFactory},
    runtime_lease::HeldRuntimeLease,
    tokenrouter, vercel,
};

pub(crate) type ProviderDiscoveryFuture<'a> =
    Pin<Box<dyn Future<Output = ProviderAvailability> + Send + 'a>>;
pub(crate) type ProviderDiscovery =
    for<'a> fn(ProviderAvailability, &'a CancellationToken) -> ProviderDiscoveryFuture<'a>;
pub(crate) type ProviderLaunch =
    for<'a> fn(
        &'a ProductionDriverFactory,
        &'a DurableAgentSession,
        &'a HeldRuntimeLease,
    ) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderConfigurationAuthority {
    Catalog,
    CallerOpenAi,
}

impl ProviderConfigurationAuthority {
    pub(crate) const fn custom_endpoint(self) -> bool {
        matches!(self, Self::CallerOpenAi)
    }

    pub(crate) const fn custom_model(self) -> bool {
        matches!(self, Self::CallerOpenAi)
    }
}

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
    pub(crate) configuration_authority: ProviderConfigurationAuthority,
    pub(crate) discover: ProviderDiscovery,
    pub(crate) launch: ProviderLaunch,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_antigravity_registered,
    launch: launch_antigravity,
};

pub(crate) static CLAUDE_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "claude",
    display_name: "Claude Code",
    provider_kind: "claude_code",
    runtime_kind: "live_cli",
    transport: "agent_sdk_stdio",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "claude",
    credential_available: false,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_claude_registered,
    launch: launch_claude,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_opencode_registered,
    launch: launch_opencode,
};

pub(crate) static CURSOR_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "cursor",
    display_name: "Cursor",
    provider_kind: "cursor_live_session",
    runtime_kind: "live_cli",
    transport: "acp_stdio",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "cursor-agent",
    credential_available: false,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_cursor_registered,
    launch: launch_cursor,
};

pub(crate) static GROK_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "grok",
    display_name: "Grok",
    provider_kind: "grok_live_session",
    runtime_kind: "live_cli",
    transport: "acp_stdio",
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "grok",
    credential_available: false,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_grok_registered,
    launch: launch_grok,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
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
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_tokenrouter_registered,
    launch: launch_tokenrouter,
};

pub(crate) static CUSTOM_API_PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "custom_api",
    display_name: custom_api::DISPLAY_NAME,
    provider_kind: custom_api::PROVIDER_KIND,
    runtime_kind: "api",
    transport: "https",
    catalog_group: "api",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: false,
    probe_executable: "",
    credential_available: true,
    configuration_authority: ProviderConfigurationAuthority::CallerOpenAi,
    discover: discover_custom_api_registered,
    launch: launch_custom_api,
};

static PROVIDER_REGISTRATIONS: [&ProviderRegistration; 16] = [
    &CODEX_PROVIDER,
    &ANTIGRAVITY_PROVIDER,
    &CLAUDE_PROVIDER,
    &OPENCODE_PROVIDER,
    &CURSOR_PROVIDER,
    &freebuff::PROVIDER,
    &GROK_PROVIDER,
    &DEEPSEEK_PROVIDER,
    &CEREBRAS_PROVIDER,
    &OPENROUTER_PROVIDER,
    &VERCEL_PROVIDER,
    &LLM_GATEWAY_PROVIDER,
    &TOKENROUTER_PROVIDER,
    &CUSTOM_API_PROVIDER,
    &ollama::PROVIDER,
    &lm_studio::PROVIDER,
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

fn discover_claude_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(crate::claude::discover(provider, cancellation))
}

fn discover_opencode_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_opencode(provider, cancellation))
}

fn discover_cursor_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(cursor::discover(provider, cancellation))
}

fn discover_grok_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(grok::discover(provider, cancellation))
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

fn discover_custom_api_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover_custom_api(provider, cancellation))
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
        custom_endpoint: registration.configuration_authority.custom_endpoint(),
        custom_model: registration.configuration_authority.custom_model(),
        controls: Vec::new(),
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
        let driver = CodexDriver::spawn(session, runtime_lease, factory.guardian()?).await?;
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
            let driver =
                AntigravityDriver::spawn(session, runtime_lease, factory.guardian()?).await?;
            Ok(Box::new(driver) as Box<dyn ProviderDriver>)
        }
        #[cfg(windows)]
        {
            let driver = AntigravityDriver::spawn(session, factory.companion()?).await?;
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

fn launch_claude<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    #[cfg(windows)]
    let _ = (factory, runtime_lease);
    #[cfg(not(any(unix, windows)))]
    let _ = (factory, session, runtime_lease);
    Box::pin(async move {
        #[cfg(unix)]
        let driver =
            ClaudeAgentSdkDriver::spawn(session, runtime_lease, factory.guardian()?).await?;
        #[cfg(windows)]
        let driver = ClaudeAgentSdkDriver::spawn(session).await?;
        #[cfg(any(unix, windows))]
        return Ok(Box::new(driver) as Box<dyn ProviderDriver>);
        #[cfg(not(any(unix, windows)))]
        Err(DriverError::new(
            "provider_runtime_unsupported",
            "Claude Agent SDK processes are unsupported on this platform.",
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
        let driver = OpenCodeDriver::spawn(session, runtime_lease, factory.guardian()?).await?;
        #[cfg(not(unix))]
        let driver = OpenCodeDriver::spawn(session).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_cursor<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    #[cfg(not(unix))]
    let _ = (factory, runtime_lease);
    Box::pin(async move {
        #[cfg(unix)]
        let driver = CursorAcpDriver::spawn(session, runtime_lease, factory.guardian()?).await?;
        #[cfg(not(unix))]
        let driver = CursorAcpDriver::spawn(session).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn launch_grok<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        let state_root = factory.state_root.as_deref().ok_or_else(|| {
            DriverLaunchError::safe(DriverError::new(
                "provider_state_unavailable",
                "Private Grok provider state is unavailable.",
            ))
        })?;
        #[cfg(unix)]
        let driver =
            GrokAcpDriver::spawn(session, runtime_lease, factory.guardian()?, state_root).await?;
        #[cfg(not(unix))]
        let driver = GrokAcpDriver::spawn(session, state_root).await?;
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

fn launch_custom_api<'a>(
    factory: &'a ProductionDriverFactory,
    session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        factory
            .credentials
            .secret(ProviderCredentialId::CustomApi)
            .await
            .map_err(|error| DriverLaunchError::safe(custom_api::credential_error(error)))?;
        let driver = custom_api::launch(factory.credentials.clone(), session).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}
