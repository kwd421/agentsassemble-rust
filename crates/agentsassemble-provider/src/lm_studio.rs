use std::{collections::HashSet, time::Duration};

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        MAX_OPTION_VALUE_BYTES, control, failed_provider, option, permission_control,
        provider_executable, ready_provider, unavailable_provider,
    },
    driver::{DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    local_openai::{model_label, request_payload},
    process::{ProbeFailure, probe_with_timeout},
    provider_factory::ProductionDriverFactory,
    registration::{ProviderConfigurationAuthority, ProviderDiscoveryFuture, ProviderRegistration},
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
        ResponseModelIdentity,
    },
    runtime_lease::HeldRuntimeLease,
};

const PREFERRED_MODEL: &str = "gemma-4-e4b-it";
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const MODEL_TIMEOUT: Duration = Duration::from_secs(8);

pub(crate) static PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "lmstudio",
    display_name: "LM Studio",
    provider_kind: "lmstudio_api",
    runtime_kind: "api",
    transport: "http",
    catalog_group: "local",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "lms",
    credential_available: false,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_registered,
    launch: launch_registered,
};

static SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Unauthenticated,
    provider_kind: "lmstudio_api",
    endpoint: RemoteOpenAiEndpoint::FixedLoopback("http://127.0.0.1:1234/v1/chat/completions"),
    headers: &[],
    request_payload,
    response_model: ResponseModelIdentity::Requested,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded LM Studio request context is too large.",
        rate_limited: "LM Studio rate-limited the request.",
        invalid_response: "LM Studio returned an invalid bounded response.",
        invalid_tool_call: "LM Studio returned an invalid room-tool call.",
        api_unavailable: "The LM Studio API request did not complete.",
        session_mismatch: "LM Studio runtime authority does not match the Agent Session.",
        session_changed: "LM Studio session authority changed after attachment.",
        already_bound: "LM Studio driver is already bound to another Agent Session.",
        room_action_missing: "LM Studio ended without the required room action.",
        tool_round_limit: "LM Studio exceeded the bounded room-tool rounds.",
        room_read_missing: "LM Studio did not perform the required room read.",
        interrupt_uncertain: "The LM Studio room action may have completed before interruption.",
    },
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
    let (executable, identity) = match provider_executable("lms", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    provider.executable_identity = identity;
    let status =
        match probe_with_timeout(&executable, &["status"], STATUS_TIMEOUT, cancellation).await {
            Ok(status) => status,
            Err(failure) => return failed_provider(provider, failure),
        };
    if !server_is_running(&status) {
        return unavailable_provider(
            provider,
            true,
            "local_server_unavailable",
            "The LM Studio local server is not running.",
        );
    }
    let output =
        match probe_with_timeout(&executable, &["ps", "--json"], MODEL_TIMEOUT, cancellation).await
        {
            Ok(output) => output,
            Err(failure) => return failed_provider(provider, failure),
        };
    let Ok(models) = tool_models(&output) else {
        return failed_provider(provider, ProbeFailure::Malformed);
    };
    if models.is_empty() {
        return unavailable_provider(
            provider,
            true,
            "no_supported_models",
            "LM Studio has no loaded model with room-tool support.",
        );
    }
    let default_model = models
        .iter()
        .find(|model| model.as_str() == PREFERRED_MODEL)
        .unwrap_or(&models[0])
        .clone();
    let options = models
        .into_iter()
        .map(|model| option(&model, &model_label(&model)))
        .collect();
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", options, &default_model),
            permission_control(false),
        ],
    )
}

fn launch_registered<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        let driver = RemoteOpenAiDriver::launch(&SPEC, factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

fn server_is_running(output: &str) -> bool {
    output.lines().any(|line| {
        line.strip_prefix("Server:")
            .and_then(|status| status.split_whitespace().next())
            == Some("ON")
    })
}

fn tool_models(output: &str) -> Result<Vec<String>, ()> {
    let payload = serde_json::from_str::<Value>(output).map_err(|_| ())?;
    let entries = payload.as_array().ok_or(())?;
    let mut seen = HashSet::new();
    Ok(entries
        .iter()
        .filter(|entry| {
            entry.get("type").and_then(Value::as_str) == Some("llm")
                && entry.get("trainedForToolUse").and_then(Value::as_bool) == Some(true)
        })
        .filter_map(|entry| {
            let identifier = entry
                .get("identifier")
                .or_else(|| entry.get("modelKey"))?
                .as_str()?
                .trim();
            (!identifier.is_empty()
                && identifier.len() <= MAX_OPTION_VALUE_BYTES
                && !identifier.chars().any(char::is_control)
                && seen.insert(identifier.to_owned()))
            .then(|| identifier.to_owned())
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::{server_is_running, tool_models};

    #[test]
    fn server_status_requires_the_owned_status_line() {
        assert!(server_is_running("Server: ON\nPort: 1234\n"));
        assert!(!server_is_running("Prefix Server: ON\n"));
        assert!(!server_is_running("Server: OFF\n"));
    }

    #[test]
    fn inventory_keeps_loaded_tool_models_and_preserves_order() {
        let models = tool_models(
            r#"[
                {"type":"llm","identifier":"gemma-4-e4b-it","trainedForToolUse":true},
                {"type":"llm","modelKey":"fallback-model","trainedForToolUse":true},
                {"type":"llm","identifier":"plain-model","trainedForToolUse":false},
                {"type":"embedding","identifier":"embed","trainedForToolUse":true},
                {"type":"llm","identifier":"gemma-4-e4b-it","trainedForToolUse":true}
            ]"#,
        )
        .unwrap_or_else(|()| panic!("parse fixture inventory"));
        assert_eq!(models, ["gemma-4-e4b-it", "fallback-model"]);
        assert!(tool_models("not json").is_err());
        assert!(tool_models("{}").is_err());
    }
}
