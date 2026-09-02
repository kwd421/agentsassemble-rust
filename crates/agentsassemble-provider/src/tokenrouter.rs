use std::collections::BTreeMap;

use agentsassemble_domain::{DurableAgentSession, ProviderControlOption};
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_catalog::{
        RemoteCatalogError, bound_catalog_options, bounded_catalog_text, catalog_model_family,
    },
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec},
};

pub(crate) const DISPLAY_NAME: &str = "TokenRouter";
pub(crate) const PROVIDER_KIND: &str = "tokenrouter_api";
pub(crate) const PREFERRED_MODEL: &str = "moonshotai/kimi-k3-free";
pub(crate) const CATALOG_ENDPOINT: &str =
    "https://tokenrouter-backend-api.tokenrouter.com/backend-api/api/pricing?sort_type=5";

pub(crate) static TOKENROUTER_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    credential: ProviderCredentialId::TokenRouter,
    provider_kind: PROVIDER_KIND,
    endpoint: RemoteOpenAiEndpoint::Fixed("https://api.tokenrouter.com/v1/chat/completions"),
    headers: &[],
    request_payload,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        credential_required: "A TokenRouter credential is required.",
        credential_invalid: "The configured TokenRouter credential is invalid.",
        credential_rejected: "TokenRouter rejected the configured credential.",
        context_limit: "The bounded TokenRouter request context is too large.",
        rate_limited: "TokenRouter rate-limited the request.",
        invalid_response: "TokenRouter returned an invalid bounded response.",
        invalid_tool_call: "TokenRouter returned an invalid room-tool call.",
        api_unavailable: "The TokenRouter request did not complete.",
        session_mismatch: "TokenRouter runtime authority does not match the Agent Session.",
        session_changed: "TokenRouter session authority changed after attachment.",
        already_bound: "TokenRouter driver is already bound to another Agent Session.",
        room_action_missing: "TokenRouter ended without the required room action.",
        tool_round_limit: "TokenRouter exceeded the bounded room-tool rounds.",
        room_read_missing: "TokenRouter did not perform the required room read.",
        interrupt_uncertain: "The TokenRouter room action may have completed before interruption.",
    },
};

pub(crate) fn model_options(
    payload: &Value,
) -> Result<Vec<ProviderControlOption>, RemoteCatalogError> {
    let entries = payload
        .get("data")
        .and_then(Value::as_array)
        .ok_or(RemoteCatalogError::Malformed)?;
    let options = entries
        .iter()
        .filter_map(tokenrouter_model_option)
        .collect();
    bound_catalog_options(options)
}

fn tokenrouter_model_option(entry: &Value) -> Option<ProviderControlOption> {
    let entry = entry.as_object()?;
    let model_id = bounded_catalog_text(entry.get("model_name")?, 128)?;
    if !bounded_catalog_text(entry.get("tags")?, 64)?.eq_ignore_ascii_case("text")
        || !contains_text(entry.get("supported_endpoint_types"), "openai")
        || !contains_text(entry.get("enable_groups"), "default")
    {
        return None;
    }
    let mut metadata = BTreeMap::from([(
        "description".to_owned(),
        json!("TokenRouter 공개 catalog · OpenAI chat"),
    )]);
    if let Some(family) = catalog_model_family(&model_id) {
        metadata.insert("family".to_owned(), json!(family));
    }
    Some(ProviderControlOption {
        value: model_id.clone(),
        label: model_id,
        metadata,
    })
}

fn contains_text(value: Option<&Value>, expected: &str) -> bool {
    value
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|value| bounded_catalog_text(value, 64))
        .any(|value| value.eq_ignore_ascii_case(expected))
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

pub(crate) const fn credential_error(error: ProviderCredentialError) -> DriverError {
    TOKENROUTER_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverError> {
    RemoteOpenAiDriver::launch(&TOKENROUTER_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CATALOG_ENDPOINT, DISPLAY_NAME, PREFERRED_MODEL, TOKENROUTER_SPEC, model_options,
        request_payload,
    };
    use crate::{remote_catalog::RemoteCatalogError, test_support::durable_session};

    #[test]
    fn public_pricing_catalog_keeps_only_default_openai_text_models() {
        let options = model_options(&json!({
            "data": [
                {
                    "model_name": "z-ai/glm-5.3-free",
                    "tags": "Text",
                    "enable_groups": ["default", "vip"],
                    "supported_endpoint_types": ["openai"]
                },
                {
                    "model_name": "image-only",
                    "tags": "Image",
                    "enable_groups": ["default"],
                    "supported_endpoint_types": ["openai"]
                },
                {
                    "model_name": "private-group",
                    "tags": "Text",
                    "enable_groups": ["vip"],
                    "supported_endpoint_types": ["openai"]
                },
                {
                    "model_name": "different-endpoint",
                    "tags": "Text",
                    "enable_groups": ["default"],
                    "supported_endpoint_types": ["anthropic"]
                }
            ]
        }))
        .unwrap_or_else(|error| panic!("project TokenRouter catalog: {error:?}"));

        assert_eq!(options.len(), 1);
        assert_eq!(options[0].value, "z-ai/glm-5.3-free");
        assert_eq!(options[0].metadata["family"], json!("Z Ai"));
        assert_eq!(
            options[0].metadata["description"],
            json!("TokenRouter 공개 catalog · OpenAI chat")
        );
        assert_eq!(
            model_options(&json!({"models": []})),
            Err(RemoteCatalogError::Malformed)
        );
    }

    #[test]
    fn request_profile_preserves_tokenrouter_controls() {
        let mut session = durable_session(
            "room",
            "tokenrouter-session",
            DISPLAY_NAME,
            "tokenrouter_api",
            "z-ai/glm-5.3-free",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "z-ai/glm-5.3-free",
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert_eq!(PREFERRED_MODEL, "moonshotai/kimi-k3-free");
        assert!(TOKENROUTER_SPEC.headers.is_empty());
        assert!(!TOKENROUTER_SPEC.retain_reasoning);
        assert!(CATALOG_ENDPOINT.starts_with("https://tokenrouter-backend-api.tokenrouter.com/"));
    }
}
