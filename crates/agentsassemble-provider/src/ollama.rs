#[cfg(unix)]
use crate::runtime_lease::HeldRuntimeLease;
use std::{collections::HashSet, time::Duration};

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability, ProviderControlOption};
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::{
    catalog::{
        MAX_OPTION_VALUE_BYTES, control, failed_provider, option, permission_control,
        provider_executable, ready_provider, unavailable_provider,
    },
    driver::{DriverFuture, ProviderDriver},
    launch_error::DriverLaunchError,
    local_openai::{model_label, request_payload},
    process::probe_with_timeout,
    provider_factory::ProductionDriverFactory,
    registration::{ProviderConfigurationAuthority, ProviderDiscoveryFuture, ProviderRegistration},
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
        ResponseModelIdentity,
    },
};

const PREFERRED_MODEL: &str = "nemotron-3-super:cloud";
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MODEL_PROBES: usize = 32;

pub(crate) static PROVIDER: ProviderRegistration = ProviderRegistration {
    id: "ollama",
    display_name: "Ollama",
    provider_kind: "ollama_api",
    runtime_kind: "api",
    transport: "http",
    catalog_group: "harness",
    workspace_required: false,
    connection_kind: "native_cli_bridge",
    executable_required: true,
    probe_executable: "ollama",
    remote_spec: None,
    turn_interrupt: agentsassemble_domain::ProviderTurnInterrupt::Unsupported,
    configuration_authority: ProviderConfigurationAuthority::Catalog,
    discover: discover_registered,
    launch: launch_registered,
};

static SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Unauthenticated,
    provider_kind: "ollama_api",
    endpoint: RemoteOpenAiEndpoint::FixedLoopback("http://127.0.0.1:11434/v1/chat/completions"),
    headers: &[],
    request_payload,
    response_model: ResponseModelIdentity::Requested,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded Ollama request context is too large.",
        rate_limited: "Ollama rate-limited the request.",
        invalid_response: "Ollama returned an invalid bounded response.",
        invalid_tool_call: "Ollama returned an invalid room-tool call.",
        api_unavailable: "The Ollama API request did not complete.",
        session_mismatch: "Ollama runtime authority does not match the Agent Session.",
        session_changed: "Ollama session authority changed after attachment.",
        already_bound: "Ollama driver is already bound to another Agent Session.",
        room_action_missing: "Ollama ended without the required room action.",
        tool_round_limit: "Ollama exceeded the bounded room-tool rounds.",
        room_read_missing: "Ollama did not perform the required room read.",
        interrupt_uncertain: "The Ollama room action may have completed before interruption.",
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
    let (executable, identity) = match provider_executable("ollama", cancellation).await {
        Ok(authority) => authority,
        Err(failure) => return failed_provider(provider, failure),
    };
    provider.executable.clone_from(&executable);
    provider.executable_identity = identity;
    let output = match probe_with_timeout(
        &executable,
        &["list"],
        DISCOVERY_TIMEOUT,
        cancellation,
        &[],
    )
    .await
    {
        Ok(output) => output,
        Err(failure) => return failed_provider(provider, failure),
    };
    let mut models = Vec::new();
    for candidate in model_entries(&output).into_iter().take(MAX_MODEL_PROBES) {
        let details = match probe_with_timeout(
            &executable,
            &["show", &candidate.value],
            DISCOVERY_TIMEOUT,
            cancellation,
            &[],
        )
        .await
        {
            Ok(details) => details,
            Err(failure) => return failed_provider(provider, failure),
        };
        if supports_tools(&details) {
            models.push(model_option(&candidate));
        }
    }
    if models.is_empty() {
        return unavailable_provider(
            provider,
            true,
            "no_supported_models",
            "Ollama has no installed model with room-tool support.",
        );
    }
    let default_model = models
        .iter()
        .find(|model| model.value == PREFERRED_MODEL)
        .unwrap_or(&models[0])
        .value
        .clone();
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            permission_control(false),
        ],
    )
}

fn launch_registered<'a>(
    factory: &'a ProductionDriverFactory,
    _session: &'a DurableAgentSession,
    #[cfg(unix)] _runtime_lease: &'a HeldRuntimeLease,
) -> DriverFuture<'a, Result<Box<dyn ProviderDriver>, DriverLaunchError>> {
    Box::pin(async move {
        let driver = RemoteOpenAiDriver::launch(&SPEC, factory.credentials.clone()).await?;
        Ok(Box::new(driver) as Box<dyn ProviderDriver>)
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelEntry {
    value: String,
    cloud: bool,
}

fn model_entries(output: &str) -> Vec<ModelEntry> {
    let mut seen = HashSet::new();
    output
        .lines()
        .filter_map(|line| {
            let columns = line.split_whitespace().collect::<Vec<_>>();
            let value = *columns.first()?;
            if value.eq_ignore_ascii_case("name")
                || value.len() > MAX_OPTION_VALUE_BYTES
                || value.chars().any(char::is_control)
                || !seen.insert(value.to_owned())
            {
                return None;
            }
            Some(ModelEntry {
                value: value.to_owned(),
                cloud: columns.get(2) == Some(&"-"),
            })
        })
        .collect()
}

fn supports_tools(output: &str) -> bool {
    let mut capabilities = false;
    for line in output.lines() {
        let normalized = line.trim().to_ascii_lowercase();
        if normalized == "capabilities" {
            capabilities = true;
        } else if capabilities && normalized == "tools" {
            return true;
        } else if capabilities
            && !normalized.is_empty()
            && !line.starts_with(' ')
            && !line.starts_with('\t')
        {
            capabilities = false;
        }
    }
    false
}

fn model_option(entry: &ModelEntry) -> ProviderControlOption {
    let mut result = option(&entry.value, &ollama_model_label(&entry.value));
    let group = if entry.cloud { "harness" } else { "local" };
    result
        .metadata
        .insert("catalog_group".to_owned(), json!(group));
    result.metadata.insert(
        "execution_location".to_owned(),
        json!(if entry.cloud { "cloud" } else { "local" }),
    );
    if entry.cloud {
        result
            .metadata
            .insert("pricing".to_owned(), json!("free_tier"));
    }
    result
}

fn ollama_model_label(value: &str) -> String {
    let (base, tag) = value.split_once(':').unwrap_or((value, ""));
    let mut normalized = String::with_capacity(base.len() + 2);
    let mut previous = None;
    for character in base.chars() {
        if character.is_ascii_digit()
            && previous.is_some_and(|value: char| value.is_ascii_alphabetic())
        {
            normalized.push('-');
        }
        normalized.push(character);
        previous = Some(character);
    }
    let mut label = model_label(&normalized);
    if !tag.is_empty() && !matches!(tag.to_ascii_lowercase().as_str(), "cloud" | "latest") {
        label.push(' ');
        let is_size = tag.strip_suffix(['b', 'B']).is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        });
        if is_size {
            label.push_str(&tag.to_ascii_uppercase());
        } else {
            label.push_str(tag);
        }
    }
    label
}

#[cfg(test)]
mod tests {
    use super::{
        ModelEntry, SPEC, model_entries, model_option, ollama_model_label, supports_tools,
    };
    use crate::{
        credentials::ProviderCredentialStore, driver::ProviderDriver,
        remote_openai::RemoteOpenAiDriver, test_support::durable_session,
    };

    #[test]
    fn inventory_preserves_location_and_tool_capability() {
        assert_eq!(
            model_entries(
                "NAME                    ID           SIZE\n\
                 gemma4:12b              4eb23ef      7.6 GB\n\
                 nemotron-3-super:cloud  c6398e0      -\n\
                 gemma4:12b              duplicate    7.6 GB\n",
            ),
            [
                ModelEntry {
                    value: "gemma4:12b".to_owned(),
                    cloud: false,
                },
                ModelEntry {
                    value: "nemotron-3-super:cloud".to_owned(),
                    cloud: true,
                },
            ]
        );
        assert!(supports_tools("Capabilities\n  completion\n  tools\n"));
        assert!(!supports_tools(
            "Capabilities\n  completion\nTemplate\n  tools\n"
        ));
    }

    #[test]
    fn options_project_cloud_and_local_models_without_changing_provider_identity() {
        let cloud = model_option(&ModelEntry {
            value: "nemotron-3-super:cloud".to_owned(),
            cloud: true,
        });
        let local = model_option(&ModelEntry {
            value: "gemma4:12b".to_owned(),
            cloud: false,
        });
        assert_eq!(cloud.label, "Nemotron 3 Super");
        assert_eq!(cloud.metadata["catalog_group"], "harness");
        assert_eq!(cloud.metadata["execution_location"], "cloud");
        assert_eq!(cloud.metadata["pricing"], "free_tier");
        assert_eq!(local.label, "Gemma 4 12B");
        assert_eq!(local.metadata["catalog_group"], "local");
        assert_eq!(ollama_model_label("llama3:latest"), "Llama 3");
    }

    #[tokio::test]
    async fn local_runtime_attaches_without_a_credential_or_provider_process() {
        let mut driver = RemoteOpenAiDriver::launch(&SPEC, ProviderCredentialStore::production())
            .await
            .unwrap_or_else(|error| panic!("launch local runtime: {}", error.error));
        let mut session = durable_session(
            "room",
            "ollama-session",
            "Ollama",
            "ollama_api",
            "gemma4:12b",
            "http",
        );
        session.public.runtime_kind = "api".to_owned();
        let attachment = driver
            .attach_session(&session)
            .await
            .unwrap_or_else(|error| panic!("attach local runtime: {error}"));
        assert_eq!(attachment.provider_session_id, "ollama_api-ollama-session");
        driver
            .stop()
            .await
            .unwrap_or_else(|error| panic!("stop local runtime: {error}"));
    }
}
