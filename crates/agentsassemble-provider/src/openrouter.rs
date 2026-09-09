use agentsassemble_domain::DurableAgentSession;
use serde_json::{Value, json};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    launch_error::DriverLaunchError,
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
        ResponseModelIdentity,
    },
};

// Keep both new releases and popular choices inside the existing public frame bound.
// Both pages are required: a failed newest read must not look like a fresh catalog.
pub(crate) const CATALOG_ENDPOINT: &str =
    "https://openrouter.ai/api/v1/models?supported_parameters=tools&sort=most-popular&limit=32";
const NEWEST_CATALOG_ENDPOINT: &str =
    "https://openrouter.ai/api/v1/models?supported_parameters=tools&sort=newest&limit=32";

pub(crate) async fn model_options(
    cancellation: &tokio_util::sync::CancellationToken,
) -> Result<
    Vec<agentsassemble_domain::ProviderControlOption>,
    crate::remote_catalog::RemoteCatalogError,
> {
    let (newest, popular) = tokio::try_join!(
        crate::remote_catalog::fetch_gateway_model_options(NEWEST_CATALOG_ENDPOINT, cancellation),
        crate::remote_catalog::fetch_gateway_model_options(CATALOG_ENDPOINT, cancellation),
    )?;
    merge_model_options(newest, popular)
}

fn merge_model_options(
    newest: Vec<agentsassemble_domain::ProviderControlOption>,
    popular: Vec<agentsassemble_domain::ProviderControlOption>,
) -> Result<
    Vec<agentsassemble_domain::ProviderControlOption>,
    crate::remote_catalog::RemoteCatalogError,
> {
    let mut seen = std::collections::BTreeSet::new();
    crate::remote_catalog::bound_catalog_options(
        newest
            .into_iter()
            .chain(popular)
            .filter(|model| seen.insert(model.value.clone()))
            .collect(),
    )
}

pub(crate) static OPENROUTER_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Bearer {
        credential: ProviderCredentialId::OpenRouter,
        required: "An OpenRouter API credential is required.",
        invalid: "The configured OpenRouter credential is invalid.",
        rejected: "OpenRouter rejected the configured credential.",
    },
    provider_kind: "openrouter_api",
    endpoint: RemoteOpenAiEndpoint::Fixed("https://openrouter.ai/api/v1/chat/completions"),
    headers: &[
        ("HTTP-Referer", "http://127.0.0.1:8765/"),
        ("X-Title", "AgentsAssemble"),
    ],
    request_payload,
    response_model: ResponseModelIdentity::Requested,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded OpenRouter request context is too large.",
        rate_limited: "OpenRouter rate-limited the request.",
        invalid_response: "OpenRouter returned an invalid bounded response.",
        invalid_tool_call: "OpenRouter returned an invalid room-tool call.",
        api_unavailable: "The OpenRouter API request did not complete.",
        session_mismatch: "OpenRouter runtime authority does not match the Agent Session.",
        session_changed: "OpenRouter session authority changed after attachment.",
        already_bound: "OpenRouter driver is already bound to another Agent Session.",
        room_action_missing: "OpenRouter ended without the required room action.",
        tool_round_limit: "OpenRouter exceeded the bounded room-tool rounds.",
        room_read_missing: "OpenRouter did not perform the required room read.",
        interrupt_uncertain: "The OpenRouter room action may have completed before interruption.",
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
    OPENROUTER_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
) -> Result<RemoteOpenAiDriver, DriverLaunchError> {
    RemoteOpenAiDriver::launch(&OPENROUTER_SPEC, credentials).await
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{CATALOG_ENDPOINT, OPENROUTER_SPEC, request_payload};
    use crate::test_support::durable_session;

    #[test]
    fn new_releases_are_included_even_before_becoming_popular() {
        let model = crate::catalog::option;
        let options = super::merge_model_options(
            vec![
                model("new/release", "New"),
                model("shared/model", "Current metadata"),
            ],
            vec![
                model("popular/model", "Popular"),
                model("shared/model", "Other page"),
            ],
        )
        .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(
            options
                .iter()
                .map(|item| item.value.as_str())
                .collect::<Vec<_>>(),
            ["new/release", "shared/model", "popular/model"]
        );
        assert_eq!(options[1].label, "Current metadata");
    }

    #[test]
    fn request_profile_preserves_openrouter_headers_and_controls() {
        let mut session = durable_session(
            "room",
            "openrouter-session",
            "OpenRouter",
            "openrouter_api",
            "openai/gpt-4.1-mini",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "openai/gpt-4.1-mini",
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert_eq!(
            OPENROUTER_SPEC.headers,
            &[
                ("HTTP-Referer", "http://127.0.0.1:8765/"),
                ("X-Title", "AgentsAssemble"),
            ]
        );
        assert!(CATALOG_ENDPOINT.ends_with("sort=most-popular&limit=32"));
        assert!(!OPENROUTER_SPEC.retain_reasoning);
    }
}
