use agentsassemble_domain::ProviderAvailability;
use tokio_util::sync::CancellationToken;

use crate::{
    acp_client::{AcpClient, AcpClientConfiguration},
    catalog::{await_filesystem, failed_provider, provider_executable, ready_provider},
    process::{ProbeFailure, inspect},
};

#[path = "cursor_models.rs"]
mod models;
pub(crate) use models::CursorCatalog;

pub(crate) fn client_configuration() -> AcpClientConfiguration {
    let mut configuration = AcpClientConfiguration::default();
    configuration.capabilities.meta = Some(serde_json::Map::from_iter([(
        "parameterizedModelPicker".to_owned(),
        serde_json::json!(true),
    )]));
    configuration
}

pub(crate) async fn discover(
    mut provider: ProviderAvailability,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    let (executable, _) = match provider_executable("cursor-agent", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    if let Err(failure) = check_authentication(&executable, cancellation).await {
        return failed_provider(provider, failure);
    }
    let identity = match await_filesystem(
        cancellation,
        crate::filesystem::cursor_executable_identity(executable.clone()),
    )
    .await
    {
        Ok(identity) => identity,
        Err(failure) => return failed_provider(provider, failure),
    };
    let Ok(bound) =
        crate::filesystem::bind_cursor_executable(executable.clone(), identity.clone()).await
    else {
        return failed_provider(provider, ProbeFailure::Failed);
    };
    provider.executable = executable;
    provider.executable_identity = identity;
    let catalog = inspect(
        bound.launch_path(),
        &["acp"],
        // Native initialization + model response measured 9.21 s; the old 10 s
        // exchange deadline also expired in the package. Keep a bounded 20 s
        // provider budget without changing other probes or adding retries.
        std::time::Duration::from_secs(20),
        cancellation,
        &[],
        |stdin, stdout| async move {
            let mut client = AcpClient::connect(stdin, stdout, client_configuration())
                .await
                .map_err(|_| ProbeFailure::Failed)?;
            let result = CursorCatalog::read(&mut client)
                .await
                .map_err(|_| ProbeFailure::Malformed);
            client.shutdown().await;
            result
        },
    )
    .await;
    match catalog {
        Ok(catalog) => ready_provider(provider, catalog.default_model.clone(), catalog.controls()),
        Err(failure) => failed_provider(provider, failure),
    }
}

async fn check_authentication(
    executable: &str,
    cancellation: &CancellationToken,
) -> Result<(), ProbeFailure> {
    let output = crate::process::probe(
        executable,
        &["status", "--format", "json"],
        cancellation,
        &[],
    )
    .await?;
    authentication_status(&output)
}

fn authentication_status(output: &str) -> Result<(), ProbeFailure> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Credentials {
        status: String,
        is_authenticated: bool,
        has_access_token: bool,
        has_refresh_token: bool,
    }
    let status: Credentials = serde_json::from_str(output).map_err(|_| ProbeFailure::Malformed)?;
    match (
        status.status.as_str(),
        status.is_authenticated,
        status.has_access_token,
        status.has_refresh_token,
    ) {
        ("authenticated", true, true, true) => Ok(()),
        ("unauthenticated", false, false, false)
        | ("partially-authenticated", false, true, false) => Err(ProbeFailure::Authentication),
        _ => Err(ProbeFailure::Malformed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_explicit_cursor_credential_state_requests_login() {
        for (status, authenticated, access, refresh, expected) in [
            ("authenticated", true, true, true, Ok(())),
            (
                "unauthenticated",
                false,
                false,
                false,
                Err(ProbeFailure::Authentication),
            ),
            (
                "partially-authenticated",
                false,
                true,
                false,
                Err(ProbeFailure::Authentication),
            ),
            (
                "authenticated",
                false,
                false,
                false,
                Err(ProbeFailure::Malformed),
            ),
            ("error", false, false, false, Err(ProbeFailure::Malformed)),
        ] {
            let output = serde_json::json!({ "status":status, "isAuthenticated":authenticated,
                "hasAccessToken":access, "hasRefreshToken":refresh })
            .to_string();
            assert_eq!(authentication_status(&output), expected);
        }
    }
}
