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
