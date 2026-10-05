use serde_json::Value;

use crate::runtime::DriverError;

const MAX_PROVIDER_ID_BYTES: usize = 128;

pub(crate) fn checked_provider_session_id(value: &str) -> Result<Option<&str>, DriverError> {
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() > MAX_PROVIDER_ID_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
        || value == "--last"
    {
        return Err(provider_session_mismatch());
    }
    Ok(Some(value))
}

pub(crate) fn provider_session_id_from_response(
    response: &Value,
) -> Result<Option<&str>, DriverError> {
    let candidates = [
        response
            .get("result")
            .and_then(|result| result.get("thread"))
            .and_then(|thread| thread.get("id")),
        response
            .get("result")
            .and_then(|result| result.get("threadId")),
        response
            .get("params")
            .and_then(|params| params.get("thread"))
            .and_then(|thread| thread.get("id")),
    ];
    let mut observed = None;
    for candidate in candidates {
        let Some(candidate) = response_provider_session_id(candidate)? else {
            continue;
        };
        if observed.is_some_and(|current| current != candidate) {
            return Err(provider_session_mismatch());
        }
        observed = Some(candidate);
    }
    Ok(observed)
}

fn response_provider_session_id(value: Option<&Value>) -> Result<Option<&str>, DriverError> {
    let Some(value) = value else {
        return Ok(None);
    };
    let value = value.as_str().ok_or_else(provider_session_mismatch)?;
    checked_provider_session_id(value)?.map_or_else(
        || Err(provider_session_unconfirmed()),
        |value| Ok(Some(value)),
    )
}

pub(crate) fn observed_model_id_from_response(
    response: &Value,
) -> Result<Option<&str>, DriverError> {
    let rerouted_model = if response.get("method").and_then(Value::as_str) == Some("model/rerouted")
    {
        let candidate = response
            .get("params")
            .and_then(|params| params.get("toModel"))
            .ok_or_else(provider_model_mismatch)?;
        if candidate.is_null() {
            return Err(provider_model_mismatch());
        }
        Some(candidate)
    } else {
        None
    };
    let candidates = [
        rerouted_model,
        response
            .get("result")
            .and_then(|result| result.get("model")),
        response
            .get("result")
            .and_then(|result| result.get("thread"))
            .and_then(|thread| thread.get("model")),
        response
            .get("result")
            .and_then(|result| result.get("turn"))
            .and_then(|turn| turn.get("model")),
        response
            .get("params")
            .and_then(|params| params.get("model")),
        response
            .get("params")
            .and_then(|params| params.get("thread"))
            .and_then(|thread| thread.get("model")),
        response
            .get("params")
            .and_then(|params| params.get("turn"))
            .and_then(|turn| turn.get("model")),
    ];
    let mut observed = None;
    for candidate in candidates.into_iter().flatten() {
        if candidate.is_null() {
            continue;
        }
        let candidate = candidate.as_str().ok_or_else(provider_model_mismatch)?;
        if candidate.is_empty()
            || candidate.len() > MAX_PROVIDER_ID_BYTES
            || candidate.trim() != candidate
            || candidate.chars().any(char::is_control)
        {
            return Err(provider_model_mismatch());
        }
        if observed.is_some_and(|current| current != candidate) {
            return Err(provider_model_mismatch());
        }
        observed = Some(candidate);
    }
    Ok(observed)
}

pub(crate) const fn provider_session_unconfirmed() -> DriverError {
    DriverError::new(
        "provider_session_unconfirmed",
        "The Codex app-server did not return a provider session identity.",
    )
}

pub(crate) const fn provider_session_mismatch() -> DriverError {
    DriverError::new(
        "provider_session_mismatch",
        "The Codex provider session identity did not match durable authority.",
    )
}

pub(crate) const fn provider_model_mismatch() -> DriverError {
    DriverError::new(
        "provider_model_mismatch",
        "The Codex app-server reported a different provider model.",
    )
}
