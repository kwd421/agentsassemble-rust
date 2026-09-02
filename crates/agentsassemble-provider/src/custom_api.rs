use std::net::IpAddr;

use agentsassemble_domain::DurableAgentSession;
use reqwest::Url;
use serde_json::{Value, json};
use url::{Host, Position};

use crate::{
    ProviderCredentialError, ProviderCredentialId, ProviderCredentialStore,
    driver::DriverError,
    remote_https::{custom_endpoint_client, public_unicast},
    remote_openai::RemoteOpenAiDriver,
    remote_openai_spec::{
        RemoteOpenAiAuthentication, RemoteOpenAiEndpoint, RemoteOpenAiErrors, RemoteOpenAiSpec,
    },
};

pub(crate) const DISPLAY_NAME: &str = "Custom API";
pub(crate) const PROVIDER_KIND: &str = "custom_openai_api";
const COMPLETION_SUFFIX: &str = "/chat/completions";

pub(crate) static CUSTOM_API_SPEC: RemoteOpenAiSpec = RemoteOpenAiSpec {
    authentication: RemoteOpenAiAuthentication::Bearer {
        credential: ProviderCredentialId::CustomApi,
        required: "A Custom API credential is required.",
        invalid: "The configured Custom API credential is invalid.",
        rejected: "Custom API rejected the configured credential.",
    },
    provider_kind: PROVIDER_KIND,
    endpoint: RemoteOpenAiEndpoint::AgentSession,
    headers: &[],
    request_payload,
    retain_reasoning: false,
    errors: RemoteOpenAiErrors {
        context_limit: "The bounded Custom API request context is too large.",
        rate_limited: "Custom API rate-limited the request.",
        invalid_response: "Custom API returned an invalid bounded response.",
        invalid_tool_call: "Custom API returned an invalid room-tool call.",
        api_unavailable: "The Custom API request did not complete.",
        session_mismatch: "Custom API runtime authority does not match the Agent Session.",
        session_changed: "Custom API session authority changed after attachment.",
        already_bound: "Custom API driver is already bound to another Agent Session.",
        room_action_missing: "Custom API ended without the required room action.",
        tool_round_limit: "Custom API exceeded the bounded room-tool rounds.",
        room_read_missing: "Custom API did not perform the required room read.",
        interrupt_uncertain: "The Custom API room action may have completed before interruption.",
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CustomEndpointError {
    Required,
    DirectHttps,
    EmbeddedAuthority,
    LocalNetwork,
}

impl CustomEndpointError {
    pub(crate) const fn message(self) -> &'static str {
        match self {
            Self::Required => "Custom API address is required.",
            Self::DirectHttps => "Custom API address must be a direct HTTPS URL.",
            Self::EmbeddedAuthority => {
                "Custom API address cannot contain credentials, a query, or a fragment."
            }
            Self::LocalNetwork => "Use a Local provider for loopback or local-network endpoints.",
        }
    }
}

pub(crate) fn normalize_endpoint(value: &str) -> Result<String, CustomEndpointError> {
    if value.is_empty() {
        return Err(CustomEndpointError::Required);
    }
    let parsed = Url::parse(value).map_err(|_| CustomEndpointError::DirectHttps)?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err(CustomEndpointError::DirectHttps);
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(CustomEndpointError::EmbeddedAuthority);
    }
    match parsed.host().ok_or(CustomEndpointError::DirectHttps)? {
        Host::Domain(host) if local_dns_name(host) => {
            return Err(CustomEndpointError::LocalNetwork);
        }
        Host::Ipv4(address) if !public_unicast(IpAddr::V4(address)) => {
            return Err(CustomEndpointError::LocalNetwork);
        }
        Host::Ipv6(address) if !public_unicast(IpAddr::V6(address)) => {
            return Err(CustomEndpointError::LocalNetwork);
        }
        Host::Domain(_) | Host::Ipv4(_) | Host::Ipv6(_) => {}
    }
    let mut path = parsed.path().trim_end_matches('/');
    let lowercase_path = path.to_ascii_lowercase();
    if lowercase_path.contains("http://") || lowercase_path.contains("https://") {
        return Err(CustomEndpointError::DirectHttps);
    }
    if lowercase_path.ends_with(COMPLETION_SUFFIX) {
        path = &path[..path.len() - COMPLETION_SUFFIX.len()];
        path = path.trim_end_matches('/');
    }
    Ok(format!("{}{}", &parsed[..Position::BeforePath], path))
}

fn local_dns_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.rsplit_once('.').is_some_and(|(_, suffix)| {
            suffix.eq_ignore_ascii_case("localhost") || suffix.eq_ignore_ascii_case("local")
        })
}

fn completion_endpoint(base: &str) -> Result<(reqwest::Client, Url), DriverError> {
    let normalized = normalize_endpoint(base).map_err(|_| session_mismatch())?;
    if normalized != base {
        return Err(session_mismatch());
    }
    let endpoint =
        Url::parse(&format!("{base}{COMPLETION_SUFFIX}")).map_err(|_| session_mismatch())?;
    let host = endpoint
        .host_str()
        .ok_or_else(session_mismatch)?
        .to_ascii_lowercase();
    let client = custom_endpoint_client(host).map_err(|_| {
        DriverError::new(
            "provider_api_unavailable",
            CUSTOM_API_SPEC.errors.api_unavailable,
        )
    })?;
    Ok((client, endpoint))
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
    CUSTOM_API_SPEC.credential_error(error)
}

pub(crate) async fn launch(
    credentials: ProviderCredentialStore,
    session: &DurableAgentSession,
) -> Result<RemoteOpenAiDriver, DriverError> {
    let (client, endpoint) = completion_endpoint(&session.provider_endpoint)?;
    RemoteOpenAiDriver::launch_for_session_endpoint(
        &CUSTOM_API_SPEC,
        credentials,
        session.provider_endpoint.clone(),
        client,
        endpoint,
    )
    .await
}

const fn session_mismatch() -> DriverError {
    DriverError::new(
        "provider_session_mismatch",
        CUSTOM_API_SPEC.errors.session_mismatch,
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{
        CUSTOM_API_SPEC, CustomEndpointError, completion_endpoint, normalize_endpoint,
        request_payload,
    };
    use crate::test_support::durable_session;

    #[test]
    fn endpoint_policy_normalizes_direct_public_https_only() {
        assert_eq!(
            normalize_endpoint("https://api.example.com/v1/chat/completions"),
            Ok("https://api.example.com/v1".to_owned())
        );
        assert_eq!(
            normalize_endpoint("https://api.example.com/v1/"),
            Ok("https://api.example.com/v1".to_owned())
        );
        assert_eq!(
            normalize_endpoint("http://api.example.com/v1"),
            Err(CustomEndpointError::DirectHttps)
        );
        assert_eq!(
            normalize_endpoint("https://user:secret@api.example.com/v1"),
            Err(CustomEndpointError::EmbeddedAuthority)
        );
        assert_eq!(
            normalize_endpoint("https://api.example.com/v1?target=other"),
            Err(CustomEndpointError::EmbeddedAuthority)
        );
        for endpoint in [
            "https://localhost/v1",
            "https://model.local/v1",
            "https://127.0.0.1/v1",
            "https://169.254.169.254/v1",
            "https://[::1]/v1",
        ] {
            assert_eq!(
                normalize_endpoint(endpoint),
                Err(CustomEndpointError::LocalNetwork),
                "accepted {endpoint}"
            );
        }
        assert_eq!(
            normalize_endpoint("https://wrapper.example/https://api.example/v1"),
            Err(CustomEndpointError::DirectHttps)
        );
        assert!(completion_endpoint("https://api.example.com/v1").is_ok());
        let Err(unnormalized) = completion_endpoint("https://api.example.com/v1/chat/completions")
        else {
            panic!("runtime accepted an unnormalized session endpoint");
        };
        assert_eq!(unnormalized.code, "provider_session_mismatch");
    }

    #[test]
    fn request_profile_preserves_custom_api_controls() {
        let mut session = durable_session(
            "room",
            "custom-session",
            "Custom API",
            "custom_openai_api",
            "vendor-model",
            "https",
        );
        session.public.runtime_kind = "api".to_owned();
        session.public.max_output_tokens = 4_096;
        session.provider_endpoint = "https://api.example.com/v1".to_owned();
        let messages = [json!({"role": "user", "content": "hello"})];
        let tools = [json!({"type": "function"})];

        assert_eq!(
            request_payload(&session, &messages, Some(&tools)),
            json!({
                "model": "vendor-model",
                "messages": messages,
                "max_tokens": 4096,
                "stream": true,
                "stream_options": {"include_usage": true},
                "tools": tools,
            })
        );
        assert!(CUSTOM_API_SPEC.headers.is_empty());
        assert!(!CUSTOM_API_SPEC.retain_reasoning);
    }
}
