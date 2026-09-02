use agentsassemble_domain::{DurableAgentSession, ProviderAvailability};
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        NATIVE_RECEIPT_ERROR_CODE, failed_provider, incomplete_provider, provider_executable,
    },
    driver::{DriverError, DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    registration::{
        ProductionDriverFactory, ProviderConfigurationAuthority, ProviderDiscoveryFuture,
        ProviderRegistration,
    },
    runtime_lease::HeldRuntimeLease,
};

const RECEIPT_ERROR_MESSAGE: &str =
    "Freebuff has no approved structured non-interactive session and completion receipt.";

pub(crate) static PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "freebuff",
    display_name: "Freebuff",
    provider_kind: "freebuff_live_session",
    runtime_kind: "live_cli",
    transport: if cfg!(windows) { "conpty" } else { "pty" },
    catalog_group: "harness",
    workspace_required: true,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "freebuff",
    credential_available: false,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_registered,
    launch: launch_registered,
};

fn discover_registered(
    provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderDiscoveryFuture<'_> {
    Box::pin(discover(provider, cancellation))
}

async fn discover(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, identity) = match provider_executable("freebuff", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable = executable;
    provider.executable_identity = identity;
    native_receipt_unavailable(provider)
}

fn native_receipt_unavailable(provider: ProviderAvailability) -> ProviderAvailability {
    incomplete_provider(provider, NATIVE_RECEIPT_ERROR_CODE, RECEIPT_ERROR_MESSAGE)
}

fn launch_registered<'a>(
    _factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async {
        Err(DriverError::new(NATIVE_RECEIPT_ERROR_CODE, RECEIPT_ERROR_MESSAGE).into())
    })
}

#[cfg(test)]
mod tests {
    use super::{PROVIDER, native_receipt_unavailable};
    use crate::registration::loading_provider;

    #[test]
    fn installed_tui_without_a_native_receipt_is_not_startable() {
        let provider = native_receipt_unavailable(loading_provider(&PROVIDER));

        assert!(provider.available);
        assert!(!provider.startable);
        assert_eq!(
            provider.discovery_error_code,
            "provider_native_receipt_unavailable"
        );
        assert!(provider.default_model.is_empty());
        assert!(provider.controls.is_empty());
    }
}
