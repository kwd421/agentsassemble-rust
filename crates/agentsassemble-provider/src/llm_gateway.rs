use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{RemoteOpenAiErrors, RemoteOpenAiSpec},
};

pub(crate) const DISPLAY_NAME: &str = "LLM Gateway";
pub(crate) const PROVIDER_KIND: &str = "llm_gateway_api";
pub(crate) const PREFERRED_MODEL: &str = "gpt-oss-120b";
pub(crate) const CATALOG_ENDPOINT: &str = "https://api.llmgateway.io/v1/models";
pub(crate) const REASONING_EFFORTS: [(&str, &str); 8] = [
    ("", "기본"),
    ("none", "None"),
    ("minimal", "Minimal"),
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "XHigh"),
    ("max", "Max"),
];

pub(crate) static LLM_GATEWAY_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    credential: ProviderCredentialId::LlmGateway,
    provider_kind: PROVIDER_KIND,
    endpoint: "https://api.llmgateway.io/v1/chat/completions",
    headers: &[],
    request_payload,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        credential_required: "An LLM Gateway credential is required.",
        credential_invalid: "The configured LLM Gateway credential is invalid.",
        credential_rejected: "LLM Gateway rejected the configured credential.",
        context_limit: "The bounded LLM Gateway request context is too large.",
        rate_limited: "LLM Gateway rate-limited the request.",
        invalid_response: "LLM Gateway returned an invalid bounded response.",
        invalid_tool_call: "LLM Gateway returned an invalid room-tool call.",
        api_unavailable: "The LLM Gateway request did not complete.",
        session_mismatch: "LLM Gateway runtime authority does not match the Agent Session.",
        session_changed: "LLM Gateway session authority changed after attachment.",
        already_bound: "LLM Gateway driver is already bound to another Agent Session.",
        room_action_missing: "LLM Gateway ended without the required room action.",
        tool_round_limit: "LLM Gateway exceeded the bounded room-tool rounds.",
        room_read_missing: "LLM Gateway did not perform the required room read.",
        interrupt_uncertain: "The LLM Gateway room action may have completed before interruption.",
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
    if !session.public.reasoning_effort.is_empty() {
        payload["reasoning_effort"] = json!(session.public.reasoning_effort);
    }
    if let Some(tools) = tools {
        payload["tools"] = Value::Array(tools.to_vec());
    }
    payload
}

pub(crate) const fn credential_error(error: ProviderCredentialError) -> DriverError {
    LLM_GATEWAY_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverError> {
    RemoteOpenAiDriver::launch(&LLM_GATEWAY_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CATALOG_ENDPOINT, LLM_GATEWAY_SPEC, PREFERRED_MODEL, REASONING_EFFORTS, request_payload,
    };
    use crate::test_support::durable_session;

    #[test]
    fn request_profile_preserves_llm_gateway_controls() {
        let mut session = durable_session(
            "room",
            "llm-gateway-session",
            "LLM Gateway",
            "llm_gateway_api",
            PREFERRED_MODEL,
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.reasoning_effort.clear();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": PREFERRED_MODEL,
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        session.public.reasoning_effort = "high".to_owned();
        assert_eq!(
            request_payload(&session, &messages, None)["reasoning_effort"],
            json!("high")
        );
        assert_eq!(
            REASONING_EFFORTS.map(|(value, _)| value),
            [
                "", "none", "minimal", "low", "medium", "high", "xhigh", "max"
            ]
        );
        assert_eq!(CATALOG_ENDPOINT, "https://api.llmgateway.io/v1/models");
        assert!(LLM_GATEWAY_SPEC.headers.is_empty());
        assert!(!LLM_GATEWAY_SPEC.retain_reasoning);
    }
}
