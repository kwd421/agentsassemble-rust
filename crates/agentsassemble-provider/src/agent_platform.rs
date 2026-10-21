use agentsassemble_domain::{DurableAgentSession, ProviderAvailability};
use reqwest::Url;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    catalog::{
        control, failed_provider, option, permission_control, preferred_model, ready_provider,
        remote_output_token_control, unavailable_provider,
    },
    driver::DriverError,
    launch_error::DriverLaunchError,
    process::ProbeFailure,
    remote_https::custom_endpoint_client,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
        ResponseModelIdentity,
    },
};

pub(crate) const DISPLAY_NAME: &str = "Agent Platform (Vertex AI)";
pub(crate) const PROVIDER_KIND: &str = "agent_platform_api";
const HOST: &str = "aiplatform.googleapis.com";
const DEFAULT_MODEL: &str = "google/gemini-2.5-flash-lite";
/// The model-list endpoint does not accept API keys, so the catalog is fixed.
const MODELS: [(&str, &str); 6] = [
    ("google/gemini-2.5-flash-lite", "Gemini 2.5 Flash Lite"),
    ("google/gemini-2.5-flash", "Gemini 2.5 Flash"),
    ("google/gemini-2.5-pro", "Gemini 2.5 Pro"),
    ("google/gemini-3-flash-preview", "Gemini 3 Flash Preview"),
    ("google/gemini-3.1-flash-lite", "Gemini 3.1 Flash Lite"),
    ("google/gemini-3.1-pro-preview", "Gemini 3.1 Pro Preview"),
];
const SERVICE_TIERS: [(&str, &str); 3] = [
    ("default", "기본"),
    ("flex", "Flex"),
    ("priority", "Priority"),
];
const SERVICE_TIER_HEADER: &str = "X-Vertex-AI-LLM-Shared-Request-Type";

pub(crate) static AGENT_PLATFORM_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Header {
        name: "x-goog-api-key",
        credential: ProviderCredentialId::AgentPlatform,
        required: "An Agent Platform API credential is required.",
        invalid: "The configured Agent Platform API credential is invalid.",
        rejected: "Agent Platform rejected the configured API credential.",
    },
    provider_kind: PROVIDER_KIND,
    endpoint: RemoteOpenAiEndpoint::AgentSettings,
    headers: &[],
    service_tier_header: Some(service_tier_header),
    request_payload,
    response_model: ResponseModelIdentity::Requested,
    retain_reasoning: false,
    require_tool_choice_on_observation: true,
    tool_calls_may_finish_with_stop: false,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded Agent Platform request context is too large.",
        rate_limited: "Agent Platform rate-limited the request.",
        invalid_response: "Agent Platform returned an invalid bounded response.",
        output_truncated: "Agent Platform stopped at the maximum response length.",
        invalid_tool_call: "Agent Platform returned an invalid room-tool call.",
        api_unavailable: "The Agent Platform request did not complete.",
        session_mismatch: "Agent Platform runtime authority does not match the Agent Session.",
        session_changed: "Agent Platform session authority changed after attachment.",
        already_bound: "Agent Platform driver is already bound to another Agent Session.",
        room_action_missing: "Agent Platform ended without the required room action.",
        tool_round_limit: "Agent Platform exceeded the bounded room-tool rounds.",
        room_read_missing: "Agent Platform did not perform the required room read.",
        interrupt_uncertain: "The Agent Platform room action may have completed before interruption.",
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingsError {
    Project,
    Location,
}

impl SettingsError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Project => "Agent Platform project ID is invalid.",
            Self::Location => "Agent Platform location is invalid.",
        }
    }
}

/// Validates the caller-owned project and location encoded as
/// `projects/{project}/locations/{location}`. A host or URL is never accepted.
pub(crate) fn normalize_settings(value: &str) -> Result<String, SettingsError> {
    let (project, location) = parse_settings(value)?;
    Ok(format!("projects/{project}/locations/{location}"))
}

#[allow(clippy::unnecessary_wraps)] // The canonical form is derived from the same owner.
fn parse_settings(value: &str) -> Result<(&str, &str), SettingsError> {
    let rest = value
        .strip_prefix("projects/")
        .ok_or(SettingsError::Project)?;
    let (project, location) = rest
        .split_once("/locations/")
        .ok_or(SettingsError::Project)?;
    if !valid_project(project) {
        return Err(SettingsError::Project);
    }
    if !valid_location(location) {
        return Err(SettingsError::Location);
    }
    Ok((project, location))
}

fn valid_project(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.is_empty() {
        return false;
    }
    if bytes.iter().all(u8::is_ascii_digit) {
        return bytes.len() <= 64;
    }
    (6..=30).contains(&bytes.len())
        && bytes[0].is_ascii_lowercase()
        && bytes[bytes.len() - 1].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-')
}

fn valid_location(value: &str) -> bool {
    if value == "global" {
        return true;
    }
    let mut parts = value.split('-');
    let (Some(region), Some(remainder), None) = (parts.next(), parts.next(), parts.next()) else {
        return false;
    };
    if region.is_empty() || !region.bytes().all(|byte| byte.is_ascii_lowercase()) {
        return false;
    }
    let Some(digit_at) = remainder.find(|character: char| character.is_ascii_digit()) else {
        return false;
    };
    if digit_at == 0 {
        return false;
    }
    let (letters, digits) = remainder.split_at(digit_at);
    letters.bytes().all(|byte| byte.is_ascii_lowercase())
        && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn completion_endpoint(settings: &str) -> Result<(reqwest::Client, Url), DriverError> {
    let (project, location) = parse_settings(settings).map_err(settings_error)?;
    let endpoint = Url::parse(&format!(
        "https://{HOST}/v1/projects/{project}/locations/{location}/endpoints/openapi/chat/completions"
    ))
    .map_err(|_| api_unavailable())?;
    let client = custom_endpoint_client(HOST.to_owned()).map_err(|_| api_unavailable())?;
    Ok((client, endpoint))
}

const fn settings_error(error: SettingsError) -> DriverError {
    DriverError::new("invalid_provider_endpoint", error.message())
}

const fn api_unavailable() -> DriverError {
    DriverError::new(
        "provider_api_unavailable",
        AGENT_PLATFORM_SPEC.errors.api_unavailable,
    )
}

fn service_tier_header(session: &DurableAgentSession) -> Option<(&'static str, &'static str)> {
    match session.public.service_tier.as_str() {
        "flex" => Some((SERVICE_TIER_HEADER, "flex")),
        "priority" => Some((SERVICE_TIER_HEADER, "priority")),
        _ => None,
    }
}

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
    if let Some(tools) = tools {
        payload["tools"] = Value::Array(tools.to_vec());
    }
    payload
}

pub(crate) async fn discover(
    mut provider: ProviderAvailability,
    credentials: &ProviderCredentialStore,
    cancellation: &CancellationToken,
) -> ProviderAvailability {
    if cancellation.is_cancelled() {
        return failed_provider(provider, ProbeFailure::Cancelled);
    }
    match credentials
        .status(ProviderCredentialId::AgentPlatform)
        .await
    {
        Ok(status) if status.configured => {}
        Ok(_) => return failed_provider(provider, ProbeFailure::Authentication),
        Err(_) => {
            return unavailable_provider(
                provider,
                true,
                "secure_store_unavailable",
                "Agent Platform model discovery cannot access the OS credential store.",
            );
        }
    }
    "static_manifest".clone_into(&mut provider.catalog_source);
    ready(provider)
}

fn ready(provider: ProviderAvailability) -> ProviderAvailability {
    let models = MODELS
        .into_iter()
        .map(|(value, label)| {
            let mut option = option(value, label);
            option
                .metadata
                .insert("relation_scope".to_owned(), json!("global"));
            option
        })
        .collect::<Vec<_>>();
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
            permission_control(true, true),
        ],
    )
}

pub(crate) const fn credential_error(error: ProviderCredentialError) -> DriverError {
    AGENT_PLATFORM_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
    session: &DurableAgentSession,
) -> Result<RemoteOpenAiDriver, DriverLaunchError> {
    let (client, endpoint) = completion_endpoint(&session.provider_endpoint)?;
    RemoteOpenAiDriver::launch_for_session_endpoint(
        &AGENT_PLATFORM_SPEC,
        credentials,
        session.provider_endpoint.clone(),
        client,
        endpoint,
    )
    .await
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tokio_util::sync::CancellationToken;

    use super::{
        AGENT_PLATFORM_SPEC, DEFAULT_MODEL, SERVICE_TIER_HEADER, SettingsError,
        completion_endpoint, credential_error, discover, parse_settings, request_payload,
        service_tier_header,
    };
    use crate::ProviderCredentialId;
    use crate::ProviderCredentialStore;
    use crate::registration::{AGENT_PLATFORM_PROVIDER, loading_provider};
    use crate::remote_openai_spec::{RemoteOpenAiAuthentication, RemoteOpenAiEndpoint};
    use crate::test_support::durable_session;

    fn session() -> agentsassemble_domain::DurableAgentSession {
        let mut session = durable_session(
            "room",
            "agent-platform-session",
            "Agent Platform",
            "agent_platform_api",
            DEFAULT_MODEL,
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        session
    }

    #[test]
    fn completion_url_is_built_from_the_project_and_location() {
        let (_global_client, global) =
            completion_endpoint("projects/my-project-123/locations/global")
                .unwrap_or_else(|error| panic!("build global endpoint: {error}"));
        assert_eq!(global.host_str(), Some("aiplatform.googleapis.com"));
        assert_eq!(
            global.as_str(),
            "https://aiplatform.googleapis.com/v1/projects/my-project-123/locations/global/endpoints/openapi/chat/completions"
        );

        let (_regional_client, regional) =
            completion_endpoint("projects/123456789012/locations/us-central1")
                .unwrap_or_else(|error| panic!("build regional endpoint: {error}"));
        assert_eq!(
            regional.path(),
            "/v1/projects/123456789012/locations/us-central1/endpoints/openapi/chat/completions"
        );
    }

    #[test]
    fn invalid_project_or_location_is_refused_before_any_request() {
        for value in [
            "",
            "https://aiplatform.googleapis.com/v1/projects/my-project/locations/global",
            "projects/Bad_Project/locations/global",
            "projects/ab/locations/global",
            "projects/-project-123/locations/global",
            "projects/my-project/locations/us",
            "projects/my-project/locations/us-central",
            "projects/my-project/locations/US-central1",
            "projects/my-project/locations/us-central1/extra",
        ] {
            let error = completion_endpoint(value)
                .err()
                .unwrap_or_else(|| panic!("accepted invalid settings {value}"));
            assert_eq!(error.code, "invalid_provider_endpoint");
            assert!(!error.message.contains(value) || value.is_empty());
        }
        assert_eq!(
            parse_settings("projects/my-project/locations/us"),
            Err(SettingsError::Location)
        );
        assert_eq!(
            parse_settings("projects/Bad_Project/locations/global"),
            Err(SettingsError::Project)
        );
    }

    #[test]
    fn selected_tier_sets_the_request_header_and_standard_omits_it() {
        assert!(AGENT_PLATFORM_SPEC.service_tier_header.is_some());
        assert!(AGENT_PLATFORM_SPEC.headers.is_empty());
        assert!(matches!(
            AGENT_PLATFORM_SPEC.authentication,
            RemoteOpenAiAuthentication::Header {
                name: "x-goog-api-key",
                ..
            }
        ));
        assert_eq!(
            AGENT_PLATFORM_SPEC.endpoint,
            RemoteOpenAiEndpoint::AgentSettings
        );

        let mut session = session();
        session.public.service_tier = "default".to_owned();
        assert_eq!(service_tier_header(&session), None);
        session.public.service_tier = String::new();
        assert_eq!(service_tier_header(&session), None);
        for tier in ["flex", "priority"] {
            session.public.service_tier = tier.to_owned();
            assert_eq!(
                service_tier_header(&session),
                Some((SERVICE_TIER_HEADER, tier))
            );
        }

        session.public.service_tier = "flex".to_owned();
        let payload = request_payload(&session, &[json!({"role": "user"})], None);
        assert!(
            payload.get("service_tier").is_none(),
            "the tier must travel in the header, not the body"
        );
    }

    #[test]
    fn observation_payload_requires_a_tool_call_only_when_tools_are_offered() {
        let session = session();
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        let observed = AGENT_PLATFORM_SPEC.request_body(&session, &messages, Some(&tools), true);
        assert_eq!(observed["tool_choice"], "required");
        assert!(
            AGENT_PLATFORM_SPEC
                .request_body(&session, &messages, Some(&tools), false)
                .get("tool_choice")
                .is_none()
        );
        assert!(
            AGENT_PLATFORM_SPEC
                .request_body(&session, &messages, None, true)
                .get("tool_choice")
                .is_none()
        );
    }

    #[test]
    fn credential_errors_never_echo_secret_material() {
        for error in [
            crate::ProviderCredentialError::MissingSecret,
            crate::ProviderCredentialError::InvalidSecret,
            crate::ProviderCredentialError::SecureStoreUnavailable,
        ] {
            let message = credential_error(error).message;
            assert!(!message.contains("agent-platform-secret-sentinel"));
            assert!(!message.contains("projects/"));
        }
        assert_eq!(
            credential_error(crate::ProviderCredentialError::MissingSecret).message,
            "An Agent Platform API credential is required."
        );
    }

    #[tokio::test]
    async fn discovery_exposes_the_fixed_catalog_and_default_model() {
        let credentials = ProviderCredentialStore::isolated_test_store();
        credentials
            .set(
                ProviderCredentialId::AgentPlatform,
                "agent-platform-secret-sentinel",
            )
            .await
            .unwrap_or_else(|error| panic!("store Agent Platform credential: {error}"));
        let provider = discover(
            loading_provider(&AGENT_PLATFORM_PROVIDER),
            &credentials,
            &CancellationToken::new(),
        )
        .await;

        assert!(provider.startable);
        assert_eq!(provider.id, "agent_platform");
        assert_eq!(provider.display_name, "Agent Platform (Vertex AI)");
        assert_eq!(provider.provider_kind, "agent_platform_api");
        assert_eq!(provider.catalog_source, "static_manifest");
        assert_eq!(provider.default_model, "google/gemini-2.5-flash-lite");
        let models = provider
            .controls
            .iter()
            .find(|control| control.key == "model")
            .unwrap_or_else(|| panic!("Agent Platform model control"));
        assert_eq!(
            models
                .options
                .iter()
                .map(|option| option.value.as_str())
                .collect::<Vec<_>>(),
            [
                "google/gemini-2.5-flash-lite",
                "google/gemini-2.5-flash",
                "google/gemini-2.5-pro",
                "google/gemini-3-flash-preview",
                "google/gemini-3.1-flash-lite",
                "google/gemini-3.1-pro-preview",
            ]
        );
        let tiers = provider
            .controls
            .iter()
            .find(|control| control.key == "service_tier")
            .unwrap_or_else(|| panic!("Agent Platform service tier control"));
        assert_eq!(
            tiers
                .options
                .iter()
                .map(|option| (option.value.as_str(), option.label.as_str()))
                .collect::<Vec<_>>(),
            [
                ("default", "기본"),
                ("flex", "Flex"),
                ("priority", "Priority")
            ]
        );
    }

    #[tokio::test]
    async fn missing_credential_blocks_discovery() {
        let provider = discover(
            loading_provider(&AGENT_PLATFORM_PROVIDER),
            &ProviderCredentialStore::isolated_test_store(),
            &CancellationToken::new(),
        )
        .await;
        assert!(!provider.startable);
        assert!(provider.controls.is_empty());
        assert_eq!(provider.discovery_error_code, "authentication_required");
    }
}
