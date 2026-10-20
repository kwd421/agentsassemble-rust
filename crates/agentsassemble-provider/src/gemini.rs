use std::collections::BTreeSet;

use agentsassemble_domain::{DurableAgentSession, ProviderAvailability, ProviderControlOption};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    catalog::{
        control, failed_provider, option, permission_control, preferred_model, ready_provider,
        remote_catalog_failure, remote_output_token_control, unavailable_provider,
    },
    driver::DriverError,
    launch_error::DriverLaunchError,
    process::ProbeFailure,
    remote_catalog::{RemoteCatalogError, bound_catalog_options, bounded_catalog_text},
    remote_https::{fetch_bounded_json, fixed_catalog_client},
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
        ResponseModelIdentity,
    },
};

/// Google paginates this endpoint at 50 models unless asked for more. One
/// request asks for the whole account list; a residual `nextPageToken` is
/// ignored rather than followed so discovery stays single-request and bounded.
pub(crate) const CATALOG_ENDPOINT: &str =
    "https://generativelanguage.googleapis.com/v1beta/models?pageSize=1000";
const DEFAULT_MODEL: &str = "gemini-3.5-flash";
const SERVICE_TIERS: [(&str, &str); 3] = [
    ("default", "기본"),
    ("flex", "Flex"),
    ("priority", "Priority"),
];

pub(crate) static GEMINI_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Bearer {
        credential: ProviderCredentialId::Gemini,
        required: "A Gemini API credential is required.",
        invalid: "The configured Gemini API credential is invalid.",
        rejected: "Gemini rejected the configured API credential.",
    },
    provider_kind: "gemini_api",
    endpoint: RemoteOpenAiEndpoint::Fixed(
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions",
    ),
    headers: &[],
    service_tier_header: None,
    request_payload,
    response_model: ResponseModelIdentity::Requested,
    retain_reasoning: false,
    tool_calls_may_finish_with_stop: true,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded Gemini request context is too large.",
        rate_limited: "Gemini rate-limited the request.",
        invalid_response: "Gemini returned an invalid bounded response.",
        output_truncated: "Gemini stopped at the maximum response length.",
        invalid_tool_call: "Gemini returned an invalid room-tool call.",
        api_unavailable: "The Gemini API request did not complete.",
        session_mismatch: "Gemini runtime authority does not match the Agent Session.",
        session_changed: "Gemini session authority changed after attachment.",
        already_bound: "Gemini driver is already bound to another Agent Session.",
        room_action_missing: "Gemini ended without the required room action.",
        tool_round_limit: "Gemini exceeded the bounded room-tool rounds.",
        room_read_missing: "Gemini did not perform the required room read.",
        interrupt_uncertain: "The Gemini room action may have completed before interruption.",
    },
};

fn request_payload(
    session: &DurableAgentSession,
    messages: &[Value],
    tools: Option<&[Value]>,
) -> Value {
    let mut payload = json!({
        "model": session.public.model,
        "messages": messages,
        "max_tokens": session.public.max_output_tokens,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    // An omitted service tier is the standard tier. Only the two priced
    // alternatives are sent so an old or empty value cannot change billing.
    if matches!(session.public.service_tier.as_str(), "flex" | "priority") {
        payload["service_tier"] = json!(session.public.service_tier);
    }
    if let Some(tools) = tools {
        payload["tools"] = Value::Array(tools.to_vec());
    }
    payload
}

pub(crate) async fn discover(
    provider: ProviderAvailability,
    credentials: &ProviderCredentialStore,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    if cancellation.is_cancelled() {
        return failed_provider(provider, ProbeFailure::Cancelled);
    }
    let secret = match credentials.secret(ProviderCredentialId::Gemini).await {
        Ok(secret) => secret,
        Err(ProviderCredentialError::MissingSecret | ProviderCredentialError::InvalidSecret) => {
            return failed_provider(provider, ProbeFailure::Authentication);
        }
        Err(ProviderCredentialError::SecureStoreUnavailable) => {
            return unavailable_provider(
                provider,
                true,
                "secure_store_unavailable",
                "Gemini model discovery cannot access the OS credential store.",
            );
        }
    };
    let Ok(client) = fixed_catalog_client() else {
        return failed_provider(provider, ProbeFailure::Failed);
    };
    let result = fetch_bounded_json(
        client
            .get(CATALOG_ENDPOINT)
            .header("x-goog-api-key", secret.expose()),
        128 * 1024,
        cancellation,
    )
    .await
    .and_then(model_options);
    match result {
        Ok(models) if !models.is_empty() => ready(provider, models),
        Ok(_) => unavailable_provider(
            provider,
            true,
            "no_supported_models",
            "Gemini returned no models that support generateContent.",
        ),
        Err(error) => failed_provider(provider, remote_catalog_failure(error)),
    }
}

fn ready(
    provider: ProviderAvailability,
    models: Vec<ProviderControlOption>,
) -> ProviderAvailability {
    let default_model = preferred_model(&models, DEFAULT_MODEL);
    ready_provider(
        provider,
        default_model.clone(),
        vec![
            control("model", "모델", "combobox", models, &default_model),
            control(
                "service_tier",
                "응답 속도",
                "select",
                SERVICE_TIERS
                    .into_iter()
                    .map(|(value, label)| option(value, label))
                    .collect(),
                "default",
            ),
            remote_output_token_control(),
            permission_control(true, false),
        ],
    )
}

#[derive(Deserialize)]
struct ModelList {
    models: Vec<Model>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Model {
    name: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    supported_generation_methods: Vec<String>,
}

fn model_options(payload: Value) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    let list: ModelList =
        serde_json::from_value(payload).map_err(|_| RemoteCatalogError::Malformed)?;
    let mut seen = BTreeSet::new();
    let mut options = Vec::new();
    for model in list.models {
        if !model
            .supported_generation_methods
            .iter()
            .any(|method| method == "generateContent")
        {
            continue;
        }
        let name = model.name.strip_prefix("models/").unwrap_or(&model.name);
        let id = bounded_catalog_text(&json!(name), 128).ok_or(RemoteCatalogError::Malformed)?;
        if !seen.insert(id.clone()) {
            continue;
        }
        let label =
            bounded_catalog_text(&json!(model.display_name), 256).unwrap_or_else(|| id.clone());
        let mut option = option(&id, &label);
        option
            .metadata
            .insert("relation_scope".to_owned(), json!("global"));
        options.push(option);
    }
    bound_catalog_options(options)
}

pub(crate) const fn credential_error(error: ProviderCredentialError) -> DriverError {
    GEMINI_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverLaunchError> {
    RemoteOpenAiDriver::launch(&GEMINI_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CATALOG_ENDPOINT, GEMINI_SPEC, credential_error, discover, model_options, ready,
        request_payload,
    };
    use crate::ProviderCredentialStore;
    use crate::registration::{GEMINI_PROVIDER, loading_provider};
    use crate::remote_https::fixed_catalog_client;
    use crate::test_support::durable_session;

    #[test]
    fn catalog_request_asks_for_the_whole_model_list_in_one_page() {
        let request = fixed_catalog_client()
            .unwrap_or_else(|error| panic!("build Gemini catalog client: {error}"))
            .get(CATALOG_ENDPOINT)
            .build()
            .unwrap_or_else(|error| panic!("build Gemini catalog request: {error}"));

        assert_eq!(request.url().path(), "/v1beta/models");
        assert_eq!(request.url().query(), Some("pageSize=1000"));
    }

    #[test]
    fn model_list_keeps_generate_content_and_strips_prefix() {
        let models = model_options(json!({
            "models": [
                {
                    "name": "models/gemini-3.5-flash",
                    "displayName": "Gemini 3.5 Flash",
                    "supportedGenerationMethods": ["generateContent", "countTokens"]
                },
                {
                    "name": "models/text-embedding-004",
                    "supportedGenerationMethods": ["embedContent"]
                },
                {
                    "name": "models/gemini-2.5-pro",
                    "displayName": "Gemini 2.5 Pro",
                    "supportedGenerationMethods": ["generateContent"]
                },
                {
                    "name": "gemini-2.5-flash",
                    "supportedGenerationMethods": ["generateContent"]
                },
                {
                    "name": "models/gemini-3.5-flash",
                    "supportedGenerationMethods": ["generateContent"]
                }
            ]
        }))
        .unwrap_or_else(|error| panic!("project Gemini models: {error:?}"));

        assert_eq!(
            models
                .iter()
                .map(|model| model.value.as_str())
                .collect::<Vec<_>>(),
            ["gemini-3.5-flash", "gemini-2.5-pro", "gemini-2.5-flash"]
        );
        assert_eq!(models[0].label, "Gemini 3.5 Flash");
    }

    #[test]
    fn catalog_advertises_the_api_mode_default_model_and_three_tiers() {
        let models = model_options(json!({
            "models": [{
                "name": "models/gemini-3.5-flash",
                "displayName": "Gemini 3.5 Flash",
                "supportedGenerationMethods": ["generateContent"]
            }]
        }))
        .unwrap_or_else(|error| panic!("project Gemini models: {error:?}"));
        let provider = ready(loading_provider(&GEMINI_PROVIDER), models);

        assert!(provider.startable);
        assert_eq!(provider.id, "gemini");
        assert_eq!(provider.provider_kind, "gemini_api");
        assert_eq!(provider.runtime_kind, "api");
        assert_eq!(provider.default_model, "gemini-3.5-flash");
        let tiers = provider
            .controls
            .iter()
            .find(|control| control.key == "service_tier")
            .unwrap_or_else(|| panic!("Gemini service tier control"));
        assert_eq!(tiers.default_value, "default");
        assert_eq!(
            tiers
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            ["default", "flex", "priority"]
        );
        assert_eq!(
            tiers
                .options
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            ["기본", "Flex", "Priority"]
        );
    }

    #[test]
    fn request_payload_sends_only_the_selected_priced_tier() {
        let mut session = durable_session(
            "room",
            "gemini-session",
            "Gemini",
            "gemini_api",
            "gemini-3.5-flash",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        session.public.service_tier = "default".to_owned();
        let standard = request_payload(&session, &messages, Some(&tools));
        assert!(standard.get("service_tier").is_none());
        assert_eq!(
            standard,
            json!({
                "model": "gemini-3.5-flash",
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );

        for tier in ["flex", "priority"] {
            session.public.service_tier = tier.to_owned();
            assert_eq!(
                request_payload(&session, &messages, None)["service_tier"],
                tier
            );
        }
        assert_eq!(GEMINI_SPEC.provider_kind, "gemini_api");
    }

    #[tokio::test]
    async fn missing_credential_blocks_discovery_without_echoing_secret_material() {
        let credentials = ProviderCredentialStore::isolated_test_store();
        let provider = discover(
            loading_provider(&GEMINI_PROVIDER),
            &credentials,
            &tokio_util::sync::CancellationToken::new(),
        )
        .await;
        assert!(!provider.startable);
        assert!(provider.controls.is_empty());
        assert_eq!(provider.discovery_error_code, "authentication_required");

        let missing = credential_error(crate::ProviderCredentialError::MissingSecret);
        assert_eq!(missing.code, "provider_credential_missing");
        assert_eq!(missing.message, "A Gemini API credential is required.");
        assert!(!missing.message.contains("gemini-secret-sentinel"));
    }
}
