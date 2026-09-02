use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec},
};

pub(crate) const CATALOG_ENDPOINT: &str = "https://ai-gateway.vercel.sh/v1/models";

pub(crate) static VERCEL_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    credential: ProviderCredentialId::Vercel,
    provider_kind: "vercel_ai_gateway",
    endpoint: RemoteOpenAiEndpoint::Fixed("https://ai-gateway.vercel.sh/v1/chat/completions"),
    headers: &[],
    request_payload,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        credential_required: "A Vercel AI Gateway credential is required.",
        credential_invalid: "The configured Vercel AI Gateway credential is invalid.",
        credential_rejected: "Vercel AI Gateway rejected the configured credential.",
        context_limit: "The bounded Vercel AI Gateway request context is too large.",
        rate_limited: "Vercel AI Gateway rate-limited the request.",
        invalid_response: "Vercel AI Gateway returned an invalid bounded response.",
        invalid_tool_call: "Vercel AI Gateway returned an invalid room-tool call.",
        api_unavailable: "The Vercel AI Gateway request did not complete.",
        session_mismatch: "Vercel AI Gateway runtime authority does not match the Agent Session.",
        session_changed: "Vercel AI Gateway session authority changed after attachment.",
        already_bound: "Vercel AI Gateway driver is already bound to another Agent Session.",
        room_action_missing: "Vercel AI Gateway ended without the required room action.",
        tool_round_limit: "Vercel AI Gateway exceeded the bounded room-tool rounds.",
        room_read_missing: "Vercel AI Gateway did not perform the required room read.",
        interrupt_uncertain: "The Vercel AI Gateway room action may have completed before interruption.",
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
    if let Some(tools) = tools {
        payload["tools"] = Value::Array(tools.to_vec());
    }
    payload
}

pub(crate) const fn credential_error(error: ProviderCredentialError) -> DriverError {
    VERCEL_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverError> {
    RemoteOpenAiDriver::launch(&VERCEL_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CATALOG_ENDPOINT, VERCEL_SPEC, request_payload};
    use crate::test_support::durable_session;

    #[test]
    fn request_profile_preserves_vercel_controls() {
        let mut session = durable_session(
            "room",
            "vercel-session",
            "Vercel AI Gateway",
            "vercel_ai_gateway",
            "openai/gpt-5.4-mini",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "openai/gpt-5.4-mini",
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert_eq!(CATALOG_ENDPOINT, "https://ai-gateway.vercel.sh/v1/models");
        assert!(VERCEL_SPEC.headers.is_empty());
        assert!(!VERCEL_SPEC.retain_reasoning);
    }
}
