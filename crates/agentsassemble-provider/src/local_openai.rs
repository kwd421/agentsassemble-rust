use std::{net::Ipv4Addr, time::Duration};

use agentsassemble_domain::DurableAgentSession;
use reqwest::{Client, Url, redirect::Policy};
use serde_json::{Value, json};
use url::Host;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocalOpenAiEndpointError;

pub(crate) fn fixed_loopback_completion(
    endpoint: &str,
) -> Result<(Client, Url), LocalOpenAiEndpointError> {
    let endpoint = Url::parse(endpoint).map_err(|_| LocalOpenAiEndpointError)?;
    if endpoint.scheme() != "http"
        || endpoint.host() != Some(Host::Ipv4(Ipv4Addr::LOCALHOST))
        || endpoint.port().is_none()
        || !endpoint.username().is_empty()
        || endpoint.password().is_some()
        || endpoint.query().is_some()
        || endpoint.fragment().is_some()
        || endpoint.path().is_empty()
        || endpoint.path().starts_with("//")
    {
        return Err(LocalOpenAiEndpointError);
    }
    let client = Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .read_timeout(Duration::from_mins(3))
        .no_proxy()
        .http1_only()
        .user_agent("AgentsAssemble/1.0")
        .build()
        .map_err(|_| LocalOpenAiEndpointError)?;
    Ok((client, endpoint))
}

pub(crate) fn request_payload(
    session: &DurableAgentSession,
    messages: &[Value],
    tools: Option<&[Value]>,
) -> Value {
    let mut payload = json!({
        "model": session.public.model,
        "messages": messages,
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    if session.public.max_output_tokens > 0 {
        payload["max_tokens"] = json!(session.public.max_output_tokens);
    }
    if let Some(tools) = tools {
        payload["tools"] = Value::Array(tools.to_vec());
    }
    payload
}

pub(crate) fn model_label(value: &str) -> String {
    let mut labels = Vec::new();
    for token in value.split('-') {
        let folded = token.to_ascii_lowercase();
        let sized_number = folded.strip_suffix('b').is_some_and(|number| {
            !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
        });
        let label = if matches!(folded.as_str(), "gpt" | "oss") || sized_number {
            folded.to_ascii_uppercase()
        } else {
            let mut characters = token.chars();
            characters.next().map_or_else(String::new, |first| {
                first.to_uppercase().chain(characters).collect()
            })
        };
        let merge_numeric = label.bytes().all(|byte| byte.is_ascii_digit())
            && labels
                .last()
                .is_some_and(|prior: &String| prior.bytes().all(|byte| byte.is_ascii_digit()));
        if let Some(prior) = labels.last_mut().filter(|_| merge_numeric) {
            prior.push('.');
            prior.push_str(&label);
        } else {
            labels.push(label);
        }
    }
    labels.join(" ")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{fixed_loopback_completion, model_label, request_payload};
    use crate::test_support::durable_session;

    #[test]
    fn local_transport_accepts_only_fixed_numeric_ipv4_loopback() {
        assert!(fixed_loopback_completion("http://127.0.0.1:11434/v1/chat/completions").is_ok());
        for endpoint in [
            "https://127.0.0.1:11434/v1/chat/completions",
            "http://localhost:11434/v1/chat/completions",
            "http://192.168.1.8:11434/v1/chat/completions",
            "http://user:secret@127.0.0.1:11434/v1/chat/completions",
            "http://127.0.0.1:11434/v1/chat/completions?next=remote",
        ] {
            assert!(
                fixed_loopback_completion(endpoint).is_err(),
                "accepted {endpoint}"
            );
        }
    }

    #[test]
    fn local_request_omits_unselected_output_limit() {
        let mut session = durable_session(
            "room",
            "local-session",
            "Local",
            "ollama_api",
            "gemma4:12b",
            "http",
        );
        session.public.runtime_kind = "api".to_owned();
        let messages = [json!({"role": "user", "content": "hello"})];
        assert_eq!(
            request_payload(&session, &messages, None),
            json!({
                "model": "gemma4:12b",
                "messages": messages,
                "stream": true,
                "stream_options": {"include_usage": true},
            })
        );
        session.public.max_output_tokens = 4_096;
        assert_eq!(
            request_payload(&session, &messages, None)["max_tokens"],
            4_096
        );
    }

    #[test]
    fn local_model_labels_preserve_known_acronyms_and_sizes() {
        assert_eq!(model_label("gemma-4-e4b-it"), "Gemma 4 E4b It");
        assert_eq!(model_label("gpt-oss-120b"), "GPT OSS 120B");
        assert_eq!(model_label("model-3-1"), "Model 3.1");
    }
}
