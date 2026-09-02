use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
    },
};

pub(crate) static DEEPSEEK_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Bearer {
        credential: ProviderCredentialId::DeepSeek,
        required: "A DeepSeek API credential is required.",
        invalid: "The configured DeepSeek credential is invalid.",
        rejected: "DeepSeek rejected the configured credential.",
    },
    provider_kind: "deepseek_api",
    endpoint: RemoteOpenAiEndpoint::Fixed("https://api.deepseek.com/chat/completions"),
    headers: &[],
    request_payload,
    retain_reasoning: true,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded DeepSeek request context is too large.",
        rate_limited: "DeepSeek rate-limited the request.",
        invalid_response: "DeepSeek returned an invalid bounded response.",
        invalid_tool_call: "DeepSeek returned an invalid room-tool call.",
        api_unavailable: "The DeepSeek API request did not complete.",
        session_mismatch: "DeepSeek runtime authority does not match the Agent Session.",
        session_changed: "DeepSeek session authority changed after attachment.",
        already_bound: "DeepSeek driver is already bound to another Agent Session.",
        room_action_missing: "DeepSeek ended without the required room action.",
        tool_round_limit: "DeepSeek exceeded the bounded room-tool rounds.",
        room_read_missing: "DeepSeek did not perform the required room read.",
        interrupt_uncertain: "The DeepSeek room action may have completed before interruption.",
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
        "thinking": {"type": if session.public.variant == "non_thinking" { "disabled" } else { "enabled" }},
        "reasoning_effort": session.public.reasoning_effort,
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
    DEEPSEEK_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverError> {
    RemoteOpenAiDriver::launch(&DEEPSEEK_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{DEEPSEEK_SPEC, request_payload};
    use crate::test_support::durable_session;

    #[test]
    fn request_profile_preserves_deepseek_dialect() {
        let mut session = durable_session(
            "room",
            "deepseek-session",
            "DeepSeek",
            "deepseek_api",
            "deepseek-v4-flash",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.reasoning_effort = "max".to_owned();
        session.public.variant = "non_thinking".to_owned();
        session.public.max_output_tokens = 8_192;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "deepseek-v4-flash",
                "messages": messages,
                "thinking": {"type": "disabled"},
                "reasoning_effort": "max",
                "max_tokens": 8192,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert_eq!(DEEPSEEK_SPEC.provider_kind, "deepseek_api");
        assert!(DEEPSEEK_SPEC.retain_reasoning);
    }
}
