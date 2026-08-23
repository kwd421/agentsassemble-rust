use std::time::Duration;

use reqwest::Response;
use serde_json::Value;
use thiserror::Error;

const MAX_EVENT_STREAM_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENT_LINE_BYTES: usize = 512 * 1024;
const MAX_EVENTS: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum OpenCodeEventError {
    #[error("the OpenCode event stream failed")]
    Transport,
    #[error("the OpenCode event stream exceeded its bound")]
    TooLarge,
    #[error("OpenCode requested unsupported interactive input")]
    InteractiveRequest,
    #[error("OpenCode reported a provider error")]
    Provider,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct OpenCodeTurnEvents {
    pub(crate) request_message: String,
    pub(crate) assistant_message: String,
    pub(crate) observed_model: String,
}

#[derive(Default)]
struct EventState {
    session_id: String,
    turn: OpenCodeTurnEvents,
    provider_error: bool,
}

impl EventState {
    fn new(session_id: &str) -> Self {
        Self {
            session_id: session_id.to_owned(),
            ..Self::default()
        }
    }

    fn accept(&mut self, event: &Value) -> Result<bool, OpenCodeEventError> {
        let event_type = event
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let Some(properties) = event.get("properties").and_then(Value::as_object) else {
            return Ok(false);
        };
        if properties
            .get("sessionID")
            .and_then(Value::as_str)
            .unwrap_or_default()
            != self.session_id
        {
            return Ok(false);
        }
        match event_type {
            "permission.asked" | "question.asked" => {
                return Err(OpenCodeEventError::InteractiveRequest);
            }
            "session.error" => self.provider_error = true,
            "message.updated" => {
                let Some(info) = properties.get("info").and_then(Value::as_object) else {
                    return Ok(false);
                };
                let role = info.get("role").and_then(Value::as_str).unwrap_or_default();
                let message_id =
                    clean_id(info.get("id").and_then(Value::as_str).unwrap_or_default());
                if role == "user" && self.turn.request_message.is_empty() {
                    self.turn.request_message = message_id;
                } else if role == "assistant"
                    && !message_id.is_empty()
                    && info.get("parentID").and_then(Value::as_str)
                        == Some(self.turn.request_message.as_str())
                {
                    self.turn.assistant_message = message_id;
                    self.turn.observed_model = observed_model(info);
                    if info.get("error").is_some_and(Value::is_object) {
                        self.provider_error = true;
                    }
                }
            }
            "session.idle" if !self.turn.request_message.is_empty() => {
                if self.provider_error {
                    return Err(OpenCodeEventError::Provider);
                }
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }
}

pub(crate) async fn collect_turn_events(
    mut response: Response,
    session_id: &str,
    timeout: Duration,
) -> Result<OpenCodeTurnEvents, OpenCodeEventError> {
    tokio::time::timeout(timeout, async move {
        let mut state = EventState::new(session_id);
        let mut pending = Vec::new();
        let mut total = 0_usize;
        let mut events = 0_usize;
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| OpenCodeEventError::Transport)?
        {
            total = total.saturating_add(chunk.len());
            if total > MAX_EVENT_STREAM_BYTES {
                return Err(OpenCodeEventError::TooLarge);
            }
            pending.extend_from_slice(&chunk);
            if pending.len() > MAX_EVENT_LINE_BYTES && !pending.contains(&b'\n') {
                return Err(OpenCodeEventError::TooLarge);
            }
            while let Some(index) = pending.iter().position(|byte| *byte == b'\n') {
                let mut line = pending.drain(..=index).collect::<Vec<_>>();
                while matches!(line.last(), Some(b'\n' | b'\r')) {
                    line.pop();
                }
                if line.len() > MAX_EVENT_LINE_BYTES {
                    return Err(OpenCodeEventError::TooLarge);
                }
                let Some(encoded) = line.strip_prefix(b"data:") else {
                    continue;
                };
                let event: Value = match serde_json::from_slice(trim_ascii(encoded)) {
                    Ok(event) => event,
                    Err(_) => continue,
                };
                events += 1;
                if events > MAX_EVENTS {
                    return Err(OpenCodeEventError::TooLarge);
                }
                if state.accept(&event)? {
                    return Ok(state.turn);
                }
            }
        }
        Err(if state.provider_error {
            OpenCodeEventError::Provider
        } else {
            OpenCodeEventError::Transport
        })
    })
    .await
    .map_err(|_| OpenCodeEventError::Transport)?
}

fn observed_model(info: &serde_json::Map<String, Value>) -> String {
    let model = info.get("model").and_then(Value::as_object);
    let provider_id = info
        .get("providerID")
        .and_then(Value::as_str)
        .or_else(|| model?.get("providerID")?.as_str())
        .unwrap_or_default();
    let model_id = info
        .get("modelID")
        .and_then(Value::as_str)
        .or_else(|| model?.get("modelID")?.as_str())
        .or_else(|| model?.get("id")?.as_str())
        .unwrap_or_default();
    if provider_id.is_empty() || model_id.is_empty() {
        String::new()
    } else {
        format!("{provider_id}/{model_id}")
    }
}

fn clean_id(value: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.len() > 128 || value.chars().any(char::is_control) {
        String::new()
    } else {
        value.to_owned()
    }
}

fn trim_ascii(mut value: &[u8]) -> &[u8] {
    while value.first().is_some_and(u8::is_ascii_whitespace) {
        value = &value[1..];
    }
    while value.last().is_some_and(u8::is_ascii_whitespace) {
        value = &value[..value.len() - 1];
    }
    value
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{EventState, OpenCodeEventError};

    #[test]
    fn turn_identity_ignores_other_sessions_and_pairs_parent_message() {
        let mut state = EventState::new("session-1");
        assert!(
            !state
                .accept(&json!({
                    "type": "message.updated",
                    "properties": {"sessionID": "other", "info": {"id": "wrong", "role": "user"}}
                }))
                .unwrap_or_else(|error| panic!("accept unrelated event: {error}"))
        );
        state
            .accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {"id": "user-1", "role": "user"}}
            }))
            .unwrap_or_else(|error| panic!("accept user event: {error}"));
        state
            .accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant",
                    "providerID": "opencode", "modelID": "hy3-free"
                }}
            }))
            .unwrap_or_else(|error| panic!("accept assistant event: {error}"));
        assert!(
            state
                .accept(&json!({"type": "session.idle", "properties": {"sessionID": "session-1"}}))
                .unwrap_or_else(|error| panic!("accept idle event: {error}"))
        );
        assert_eq!(state.turn.request_message, "user-1");
        assert_eq!(state.turn.assistant_message, "assistant-1");
        assert_eq!(state.turn.observed_model, "opencode/hy3-free");
    }

    #[test]
    fn interactive_provider_requests_fail_closed() {
        let mut state = EventState::new("session-1");
        assert_eq!(
            state.accept(&json!({
                "type": "permission.asked",
                "properties": {"sessionID": "session-1", "id": "permission-1"}
            })),
            Err(OpenCodeEventError::InteractiveRequest)
        );
    }
}
