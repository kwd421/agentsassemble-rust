use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{RemoteOpenAiErrors, RemoteOpenAiSpec},
};

pub(crate) const CATALOG_ENDPOINT: &str =
    "https://api.cerebras.ai/public/v1/models?format=openrouter";

pub(crate) static CEREBRAS_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    credential: ProviderCredentialId::Cerebras,
    provider_kind: "cerebras_api",
    endpoint: "https://api.cerebras.ai/v1/chat/completions",
    headers: &[("X-Cerebras-Version-Patch", "2")],
    request_payload,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        credential_required: "A Cerebras API credential is required.",
        credential_invalid: "The configured Cerebras credential is invalid.",
        credential_rejected: "Cerebras rejected the configured credential.",
        context_limit: "The bounded Cerebras request context is too large.",
        rate_limited: "Cerebras rate-limited the request.",
        invalid_response: "Cerebras returned an invalid bounded response.",
        invalid_tool_call: "Cerebras returned an invalid room-tool call.",
        api_unavailable: "The Cerebras API request did not complete.",
        session_mismatch: "Cerebras runtime authority does not match the Agent Session.",
        session_changed: "Cerebras session authority changed after attachment.",
        already_bound: "Cerebras driver is already bound to another Agent Session.",
        room_action_missing: "Cerebras ended without the required room action.",
        tool_round_limit: "Cerebras exceeded the bounded room-tool rounds.",
        room_read_missing: "Cerebras did not perform the required room read.",
        interrupt_uncertain: "The Cerebras room action may have completed before interruption.",
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
    CEREBRAS_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverError> {
    RemoteOpenAiDriver::launch(&CEREBRAS_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CEREBRAS_SPEC, request_payload};
    use crate::test_support::durable_session;

    #[test]
    fn request_profile_preserves_cerebras_header_and_controls() {
        let mut session = durable_session(
            "room",
            "cerebras-session",
            "Cerebras",
            "cerebras_api",
            "gpt-oss-120b",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.reasoning_effort = "high".to_owned();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "gpt-oss-120b",
                "messages": messages,
                "reasoning_effort": "high",
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert_eq!(CEREBRAS_SPEC.headers, &[("X-Cerebras-Version-Patch", "2")]);
        assert!(!CEREBRAS_SPEC.retain_reasoning);
    }
}
