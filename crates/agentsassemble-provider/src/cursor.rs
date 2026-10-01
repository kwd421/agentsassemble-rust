use agentsassemble_domain::ProviderAvailability;
use tokio_util::sync::CancellationToken;

use crate::{
    acp_client::{AcpClient, AcpClientConfiguration, AcpPermissionPolicy, AcpToolIdentityContract},
    catalog::{await_filesystem, failed_provider, provider_executable, ready_provider},
    process::{ProbeFailure, inspect},
};

#[path = "cursor_models.rs"]
mod models;
pub(crate) use models::CursorCatalog;

pub(crate) fn client_configuration(
    permission_policy: AcpPermissionPolicy,
) -> AcpClientConfiguration {
    let mut configuration = AcpClientConfiguration {
        permission_policy,
        tool_identity: AcpToolIdentityContract::QualifiedMcpCall,
        ..Default::default()
    };
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
            let mut client = AcpClient::connect(
                stdin,
                stdout,
                client_configuration(AcpPermissionPolicy::Reject),
            )
            .await
            .map_err(|_| ProbeFailure::Failed)?;
            let result = CursorCatalog::read(&mut client)
                .await
                .map_err(|error| catalog_failure(&error));
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

fn catalog_failure(error: &crate::driver::DriverError) -> ProbeFailure {
    if error.code == "authentication_required" {
        ProbeFailure::Authentication
    } else {
        ProbeFailure::Malformed
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

    #[tokio::test]
    async fn native_catalog_auth_rejection_survives_the_protocol_boundary() {
        use serde_json::{Value, json};
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        for (code, expected) in [
            (-32000, ProbeFailure::Authentication),
            (-32603, ProbeFailure::Malformed),
        ] {
            let (input, peer_input) = tokio::io::duplex(4096);
            let (mut peer_output, output) = tokio::io::duplex(4096);
            let peer = tokio::spawn(async move {
                let mut lines = BufReader::new(peer_input).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    let request: Value = serde_json::from_str(&line)
                        .unwrap_or_else(|error| panic!("request: {error}"));
                    let body = match request["method"].as_str() {
                        Some("initialize") => json!({"result":{
                            "protocolVersion":1,"agentCapabilities":{}
                        }}),
                        Some("cursor/list_available_models") => json!({"error":{
                            "code":code,"message":"private provider detail",
                            "data":{"private":"not public"}
                        }}),
                        other => panic!("unexpected request: {other:?}"),
                    };
                    let mut reply = body;
                    reply["jsonrpc"] = json!("2.0");
                    reply["id"] = request["id"].clone();
                    peer_output
                        .write_all(format!("{reply}\n").as_bytes())
                        .await
                        .unwrap_or_else(|error| panic!("reply: {error}"));
                }
            });
            let mut client = AcpClient::connect(
                input,
                output,
                client_configuration(AcpPermissionPolicy::Reject),
            )
            .await
            .unwrap_or_else(|error| panic!("connect: {:?}", error.error));
            let Err(error) = CursorCatalog::read(&mut client).await else {
                panic!("rejected catalog must fail");
            };
            assert!(!error.to_string().contains("private"));
            assert_eq!(catalog_failure(&error), expected);
            client.shutdown().await;
            peer.await.unwrap_or_else(|error| panic!("peer: {error}"));
        }
    }

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
