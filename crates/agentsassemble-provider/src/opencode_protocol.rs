use agentsassemble_domain::DurableAgentSession;
use serde_json::{Map, Value};

use crate::{
    loopback_http::LoopbackHttpError, opencode_sse::OpenCodeEventError, runtime::DriverError,
};

pub(crate) const fn turn_in_progress() -> DriverError {
    DriverError::new(
        "provider_turn_in_progress",
        "A different OpenCode turn still owns this provider session.",
    )
}

pub(crate) struct AssistantMessage {
    pub(crate) id: String,
    pub(crate) parent_id: String,
    pub(crate) content: String,
    pub(crate) observed_model: String,
}

pub(crate) fn assistant_message(value: &Value) -> Result<AssistantMessage, DriverError> {
    let info = value
        .get("info")
        .and_then(Value::as_object)
        .ok_or_else(protocol_error)?;
    if info.get("role").and_then(Value::as_str) != Some("assistant") {
        return Err(protocol_error());
    }
    let id = info
        .get("id")
        .and_then(Value::as_str)
        .and_then(clean_session_id)
        .ok_or_else(protocol_error)?;
    let parent_id = info
        .get("parentID")
        .and_then(Value::as_str)
        .and_then(clean_session_id)
        .ok_or_else(protocol_error)?;
    let content = value
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|part| part.get("text").and_then(Value::as_str))
        .collect::<String>()
        .trim()
        .to_owned();
    Ok(AssistantMessage {
        id,
        parent_id,
        content,
        observed_model: observed_model(info)?.ok_or_else(protocol_error)?,
    })
}

pub(crate) fn observed_model(info: &Map<String, Value>) -> Result<Option<String>, DriverError> {
    let nested = match info.get("model") {
        Some(value) => Some(value.as_object().ok_or_else(protocol_error)?),
        None => None,
    };
    let provider = exact_alias([
        info.get("providerID"),
        nested.and_then(|model| model.get("providerID")),
    ])?;
    let model = exact_alias([
        info.get("modelID"),
        nested.and_then(|model| model.get("modelID")),
        nested.and_then(|model| model.get("id")),
    ])?;
    match (provider, model) {
        (Some(provider), Some(model)) => Ok(Some(format!("{provider}/{model}"))),
        (None, None) => Ok(None),
        _ => Err(protocol_error()),
    }
}

fn exact_alias<const N: usize>(values: [Option<&Value>; N]) -> Result<Option<&str>, DriverError> {
    let mut observed = None;
    for value in values.into_iter().flatten() {
        let value = value.as_str().ok_or_else(protocol_error)?;
        if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
            return Err(protocol_error());
        }
        if observed.is_some_and(|current| current != value) {
            return Err(protocol_error());
        }
        observed = Some(value);
    }
    Ok(observed)
}

pub(crate) fn validate_profile(session: &DurableAgentSession) -> Result<(), DriverError> {
    let _ = provider_id(&session.public.model)?;
    let _ = model_id(&session.public.model)?;
    if !matches!(session.public.variant.as_str(), "" | "high" | "max")
        || !matches!(
            session.public.permission_mode.as_str(),
            "meeting_read_only" | "workspace_write" | "full_access"
        )
        || !session.public.reasoning_effort.is_empty()
        || !session.public.service_tier.is_empty()
    {
        return Err(profile_error());
    }
    Ok(())
}

pub(crate) fn provider_id(model: &str) -> Result<&str, DriverError> {
    let (provider, _) = split_model(model)?;
    Ok(provider)
}

pub(crate) fn model_id(model: &str) -> Result<&str, DriverError> {
    let (_, model) = split_model(model)?;
    Ok(model)
}

fn split_model(model: &str) -> Result<(&str, &str), DriverError> {
    let Some((provider, model_id)) = model.split_once('/') else {
        return Err(profile_error());
    };
    if !matches!(provider, "opencode" | "opencode-go")
        || model_id.is_empty()
        || model.len() > 128
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._:/-".contains(&byte))
    {
        return Err(profile_error());
    }
    Ok((provider, model_id))
}

pub(crate) fn clean_session_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()
        && value != "."
        && value != ".."
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)))
    .then(|| value.to_owned())
}

pub(crate) fn session_path(session_id: &str) -> Result<String, DriverError> {
    clean_session_id(session_id)
        .map(|session_id| format!("/session/{session_id}"))
        .ok_or_else(protocol_error)
}

pub(crate) fn turn_transport_error(error: impl Into<TurnTransportError>) -> DriverError {
    match error.into() {
        TurnTransportError::Http => provider_request_error(),
        TurnTransportError::Events(OpenCodeEventError::Provider(error)) => error,
        TurnTransportError::Events(
            OpenCodeEventError::Transport
            | OpenCodeEventError::TooLarge
            | OpenCodeEventError::Protocol,
        ) => protocol_error(),
    }
}

pub(crate) enum TurnTransportError {
    Http,
    Events(OpenCodeEventError),
}

impl From<LoopbackHttpError> for TurnTransportError {
    fn from(_error: LoopbackHttpError) -> Self {
        Self::Http
    }
}

impl From<OpenCodeEventError> for TurnTransportError {
    fn from(error: OpenCodeEventError) -> Self {
        Self::Events(error)
    }
}

pub(crate) const fn http_driver_error(_error: LoopbackHttpError) -> DriverError {
    DriverError::new(
        "provider_transport_failed",
        "The OpenCode loopback HTTP transport failed.",
    )
}

pub(crate) const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The selected OpenCode executable authority changed.",
    )
}

pub(crate) const fn spawn_error() -> DriverError {
    DriverError::new(
        "provider_spawn_failed",
        "The OpenCode server process could not be started.",
    )
}

pub(crate) const fn config_error() -> DriverError {
    DriverError::new(
        "provider_config_unavailable",
        "The OpenCode provider configuration could not be isolated.",
    )
}

pub(crate) const fn startup_error() -> DriverError {
    DriverError::new(
        "provider_startup_timeout",
        "The OpenCode server did not become ready.",
    )
}

pub(crate) const fn runtime_exited() -> DriverError {
    DriverError::new(
        "provider_runtime_exited",
        "The OpenCode server exited unexpectedly.",
    )
}

#[cfg(not(unix))]
pub(crate) const fn health_error() -> DriverError {
    DriverError::new(
        "provider_health_unknown",
        "The OpenCode server health could not be observed.",
    )
}

#[cfg(not(unix))]
pub(crate) const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_unconfirmed",
        "The OpenCode server shutdown could not be confirmed.",
    )
}

pub(crate) const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The stored OpenCode runtime profile is invalid.",
    )
}

pub(crate) const fn session_unconfirmed() -> DriverError {
    DriverError::new(
        "provider_session_unconfirmed",
        "OpenCode did not return a valid session identity.",
    )
}

pub(crate) const fn session_missing() -> DriverError {
    DriverError::new(
        "provider_session_missing",
        "The stored OpenCode session no longer exists.",
    )
}

pub(crate) const fn session_mismatch() -> DriverError {
    DriverError::new(
        "provider_session_mismatch",
        "The OpenCode session identity changed.",
    )
}

pub(crate) const fn provider_request_error() -> DriverError {
    DriverError::new(
        "provider_request_failed",
        "OpenCode 요청을 처리하지 못했어요.",
    )
}

// Only the structured error message is public diagnostic input. Never inspect or
// publish responseBody, responseHeaders, request payloads or the full provider envelope.
pub(crate) fn provider_response_error(value: &Value) -> DriverError {
    let error = value
        .pointer("/info/error")
        .or_else(|| value.get("error"))
        .unwrap_or(value);
    let message = error
        .pointer("/data/message")
        .or_else(|| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let guidance = if message.contains("OpenCode 1.18.0 or newer is required") {
        Some("OpenCode를 1.18 이상으로 업데이트해 주세요.")
    } else if message.contains("free tier can only be used") {
        Some(
            "OpenCode가 이 실행 방식의 무료 모델 사용을 허용하지 않았어요. OpenCode의 무료 티어 제한을 확인해 주세요.",
        )
    } else if message.to_ascii_lowercase().contains("rate limit") {
        Some("OpenCode 요청 한도에 도달했어요. 잠시 후 다시 시도해 주세요.")
    } else {
        None
    };
    if let Some(message) = guidance {
        return DriverError::new("provider_request_failed", message);
    }
    let safe = agentsassemble_domain::redact_persisted_diagnostic_text(message, 240);
    let safe = safe.split_whitespace().collect::<Vec<_>>().join(" ");
    if safe.is_empty() {
        return provider_request_error();
    }
    DriverError {
        code: "provider_request_failed".into(),
        message: format!("OpenCode 요청을 처리하지 못했어요. · {safe}").into(),
    }
}

pub(crate) const fn turn_timeout() -> DriverError {
    DriverError::new("provider_turn_timeout", "The OpenCode turn timed out.")
}

pub(crate) const fn turn_mismatch() -> DriverError {
    DriverError::new(
        "provider_turn_mismatch",
        "The OpenCode response did not match the assigned user turn.",
    )
}

pub(crate) const fn turn_empty() -> DriverError {
    DriverError::new(
        "provider_turn_empty",
        "OpenCode completed without a final assistant message.",
    )
}

pub(crate) const fn model_mismatch() -> DriverError {
    DriverError::new(
        "provider_model_mismatch",
        "OpenCode reported a different model than the selected model.",
    )
}

pub(crate) const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The OpenCode session protocol response was invalid.",
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{assistant_message, clean_session_id, split_model};

    #[test]
    fn provider_messages_are_korean_bounded_and_redacted() {
        for (message, expected) in [
            (
                "Error from provider (Console): OpenCode 1.18.0 or newer is required to use the free tier",
                "OpenCode를 1.18 이상으로 업데이트해 주세요.",
            ),
            (
                "Error from provider (Console): OpenCode's free tier can only be used from within OpenCode",
                "OpenCode가 이 실행 방식의 무료 모델 사용을 허용하지 않았어요. OpenCode의 무료 티어 제한을 확인해 주세요.",
            ),
        ] {
            for envelope in [
                json!({"error": {"data": {"message": message}}}),
                json!({"info": {"error": {"data": {"message": message}}}}),
            ] {
                assert_eq!(super::provider_response_error(&envelope).message, expected);
            }
        }
        let error = super::provider_response_error(&json!({"data": {
            "message": "Capacity unavailable. api_key=sk-test-private-value /Users/alice/private",
            "responseBody": "private response body", "responseHeaders": {"Authorization": "private header"}
        }}));
        assert!(
            error
                .message
                .starts_with("OpenCode 요청을 처리하지 못했어요. · Capacity unavailable.")
        );
        for secret in [
            "sk-test-private-value",
            "alice",
            "private response body",
            "private header",
        ] {
            assert!(!error.message.contains(secret));
        }
        assert_eq!(
            super::provider_response_error(&json!({})),
            super::provider_request_error()
        );
        let long = super::provider_response_error(&json!({"message": "x".repeat(1000)}));
        assert!(long.message.chars().count() < 280);
    }

    #[test]
    fn model_and_session_ids_are_strict_path_components() {
        assert_eq!(
            split_model("opencode/hy3-free"),
            Ok(("opencode", "hy3-free"))
        );
        assert!(split_model("other/hy3-free").is_err());
        assert!(split_model("opencode/hy3\nfree").is_err());
        assert_eq!(clean_session_id("ses_123"), Some("ses_123".to_owned()));
        assert_eq!(clean_session_id("../ses"), None);
    }

    #[test]
    fn assistant_response_requires_parent_and_collects_only_text_parts() {
        let message = assistant_message(&json!({
            "info": {
                "id": "assistant-1",
                "parentID": "user-1",
                "role": "assistant",
                "providerID": "opencode",
                "modelID": "hy3-free"
            },
            "parts": [
                {"type": "reasoning", "text": "private"},
                {"type": "text", "text": "hello"}
            ]
        }))
        .unwrap_or_else(|error| panic!("parse assistant: {error}"));
        assert_eq!(message.id, "assistant-1");
        assert_eq!(message.parent_id, "user-1");
        assert_eq!(message.content, "hello");
        assert_eq!(message.observed_model, "opencode/hy3-free");
    }

    #[test]
    fn assistant_response_requires_one_exact_nonconflicting_model_identity() {
        assert!(
            assistant_message(&json!({
                "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant"
                },
                "parts": []
            }))
            .is_err()
        );
        assert!(
            assistant_message(&json!({
                "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant",
                    "providerID": "opencode", "modelID": "hy3-free",
                    "model": {"providerID": "other", "modelID": "hy3-free"}
                },
                "parts": []
            }))
            .is_err()
        );
        assert!(
            assistant_message(&json!({
                "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant",
                    "providerID": "opencode", "modelID": "hy3-free",
                    "model": {"providerID": "opencode", "modelID": "other", "id": "hy3-free"}
                },
                "parts": []
            }))
            .is_err()
        );
    }
}
