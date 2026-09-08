use std::time::Duration;

use serde_json::Value;
use thiserror::Error;

use crate::{loopback_http::LoopbackStream, opencode_protocol::observed_model};

const MAX_EVENT_STREAM_BYTES: usize = 8 * 1024 * 1024;
const MAX_EVENT_LINE_BYTES: usize = 512 * 1024;
const MAX_EVENTS: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum OpenCodeEventError {
    #[error("the OpenCode event stream failed")]
    Transport,
    #[error("the OpenCode event stream exceeded its bound")]
    TooLarge,
    #[error("the OpenCode event stream protocol was invalid")]
    Protocol,
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
    mode: WaitMode,
    turn: OpenCodeTurnEvents,
    provider_error: bool,
    request: Option<Value>,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum WaitMode {
    #[default]
    Turn,
    Quiescence,
}

impl EventState {
    fn new(session_id: &str, mode: WaitMode) -> Self {
        Self {
            session_id: session_id.to_owned(),
            mode,
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
            "permission.asked" | "question.asked" if self.mode == WaitMode::Turn => {
                self.request = Some(event.clone());
                return Ok(false);
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
                    if let Some(model) =
                        observed_model(info).map_err(|_| OpenCodeEventError::Protocol)?
                    {
                        if !self.turn.observed_model.is_empty() && self.turn.observed_model != model
                        {
                            return Err(OpenCodeEventError::Protocol);
                        }
                        self.turn.observed_model = model;
                    }
                    if info.get("error").is_some_and(Value::is_object) {
                        self.provider_error = true;
                    }
                }
            }
            "session.status"
                if terminal_idle(event_type, properties)
                    && (self.mode == WaitMode::Quiescence
                        || !self.turn.request_message.is_empty()) =>
            {
                if self.mode == WaitMode::Turn && self.provider_error {
                    return Err(OpenCodeEventError::Provider);
                }
                return Ok(true);
            }
            _ => {}
        }
        Ok(false)
    }
}

pub(crate) enum TurnEvent {
    Request(Value),
    Completed(OpenCodeTurnEvents),
}

pub(crate) struct TurnEventStream {
    response: LoopbackStream,
    state: EventState,
    pending: Vec<u8>,
    total: usize,
    events: usize,
}

impl TurnEventStream {
    pub(crate) fn new(response: LoopbackStream, session_id: &str) -> Self {
        Self {
            response,
            state: EventState::new(session_id, WaitMode::Turn),
            pending: Vec::new(),
            total: 0,
            events: 0,
        }
    }

    // State survives cancellation by the prompt-response branch of the turn owner's select.
    pub(crate) async fn next(&mut self) -> Result<TurnEvent, OpenCodeEventError> {
        loop {
            if accept_complete_lines(&mut self.pending, &mut self.state, &mut self.events)? {
                return Ok(self.state.request.take().map_or_else(
                    || TurnEvent::Completed(std::mem::take(&mut self.state.turn)),
                    TurnEvent::Request,
                ));
            }
            let Some(chunk) = self
                .response
                .chunk()
                .await
                .map_err(|_| OpenCodeEventError::Transport)?
            else {
                return Err(if self.state.provider_error {
                    OpenCodeEventError::Provider
                } else {
                    OpenCodeEventError::Transport
                });
            };
            self.total = self.total.saturating_add(chunk.len());
            if self.total > MAX_EVENT_STREAM_BYTES {
                return Err(OpenCodeEventError::TooLarge);
            }
            self.pending.extend_from_slice(&chunk);
            if self.pending.len() > MAX_EVENT_LINE_BYTES && !self.pending.contains(&b'\n') {
                return Err(OpenCodeEventError::TooLarge);
            }
        }
    }
}

pub(crate) async fn wait_session_idle(
    response: LoopbackStream,
    session_id: &str,
    timeout: Duration,
) -> Result<(), OpenCodeEventError> {
    let mut events = TurnEventStream::new(response, session_id);
    events.state.mode = WaitMode::Quiescence;
    match tokio::time::timeout(timeout, events.next())
        .await
        .map_err(|_| OpenCodeEventError::Transport)??
    {
        TurnEvent::Completed(_) => Ok(()),
        TurnEvent::Request(_) => Err(OpenCodeEventError::Protocol),
    }
}

fn accept_complete_lines(
    pending: &mut Vec<u8>,
    state: &mut EventState,
    events: &mut usize,
) -> Result<bool, OpenCodeEventError> {
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
        let event = serde_json::from_slice(trim_ascii(encoded))
            .map_err(|_| OpenCodeEventError::Protocol)?;
        *events += 1;
        if *events > MAX_EVENTS {
            return Err(OpenCodeEventError::TooLarge);
        }
        if state.accept(&event)? || state.request.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

fn terminal_idle(event_type: &str, properties: &serde_json::Map<String, Value>) -> bool {
    event_type == "session.status"
        && properties
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            == Some("idle")
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

    use super::{EventState, OpenCodeEventError, WaitMode, accept_complete_lines};

    #[test]
    fn split_data_is_buffered_but_malformed_data_fails_immediately() {
        let encoded = format!(
            "data: {}\n",
            json!({
                "type": "message.updated",
                "properties": {
                    "sessionID": "session-1",
                    "info": {"id": "user-1", "role": "user"}
                }
            })
        );
        let split = encoded.len() / 2;
        let mut pending = encoded.as_bytes()[..split].to_vec();
        let mut state = EventState::new("session-1", WaitMode::Turn);
        let mut events = 0;
        assert_eq!(
            accept_complete_lines(&mut pending, &mut state, &mut events),
            Ok(false)
        );
        pending.extend_from_slice(&encoded.as_bytes()[split..]);
        assert_eq!(
            accept_complete_lines(&mut pending, &mut state, &mut events),
            Ok(false)
        );
        assert_eq!(state.turn.request_message, "user-1");

        pending.extend_from_slice(b"data: not-json\n");
        assert_eq!(
            accept_complete_lines(&mut pending, &mut state, &mut events),
            Err(OpenCodeEventError::Protocol)
        );
    }

    #[test]
    fn turn_identity_ignores_other_sessions_and_pairs_parent_message() {
        let mut state = EventState::new("session-1", WaitMode::Turn);
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
                .accept(&json!({
                    "type": "session.status",
                    "properties": {"sessionID": "session-1", "status": {"type": "idle"}}
                }))
                .unwrap_or_else(|error| panic!("accept idle event: {error}"))
        );
        assert_eq!(state.turn.request_message, "user-1");
        assert_eq!(state.turn.assistant_message, "assistant-1");
        assert_eq!(state.turn.observed_model, "opencode/hy3-free");
    }

    #[test]
    fn interactive_provider_requests_are_retained_for_the_turn_owner() {
        let mut state = EventState::new("session-1", WaitMode::Turn);
        assert_eq!(
            state.accept(&json!({
                "type": "permission.asked",
                "properties": {"sessionID": "session-1", "id": "permission-1"}
            })),
            Ok(false)
        );
        assert_eq!(
            state
                .request
                .as_ref()
                .and_then(|request| request.pointer("/properties/id"))
                .and_then(serde_json::Value::as_str),
            Some("permission-1")
        );
    }

    #[test]
    fn quiescence_ignores_abort_side_events_until_current_idle() {
        let mut state = EventState::new("session-1", WaitMode::Quiescence);
        for event in [
            json!({
                "type": "permission.asked",
                "properties": {"sessionID": "session-1", "id": "permission-1"}
            }),
            json!({"type": "session.error", "properties": {"sessionID": "session-1"}}),
            json!({
                "type": "session.status",
                "properties": {"sessionID": "session-1", "status": {"type": "busy"}}
            }),
        ] {
            assert!(!state.accept(&event).unwrap_or_else(|error| {
                panic!("accept non-terminal abort side event: {error}")
            }));
        }
        assert!(
            state
                .accept(&json!({
                    "type": "session.status",
                    "properties": {"sessionID": "session-1", "status": {"type": "idle"}}
                }))
                .unwrap_or_else(|error| panic!("accept terminal idle event: {error}"))
        );
    }

    #[test]
    fn model_may_arrive_late_but_cannot_change_or_remain_missing_at_completion() {
        let mut state = EventState::new("session-1", WaitMode::Turn);
        state
            .accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {
                    "id": "user-1", "role": "user"
                }}
            }))
            .unwrap_or_else(|error| panic!("accept user event: {error}"));
        state
            .accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant"
                }}
            }))
            .unwrap_or_else(|error| panic!("accept initial assistant event: {error}"));
        assert!(state.turn.observed_model.is_empty());
        state
            .accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant",
                    "model": {"providerID": "opencode", "modelID": "hy3-free"}
                }}
            }))
            .unwrap_or_else(|error| panic!("accept exact assistant model: {error}"));
        assert_eq!(state.turn.observed_model, "opencode/hy3-free");
        assert_eq!(
            state.accept(&json!({
                "type": "message.updated",
                "properties": {"sessionID": "session-1", "info": {
                    "id": "assistant-1", "parentID": "user-1", "role": "assistant",
                    "providerID": "opencode", "modelID": "other"
                }}
            })),
            Err(OpenCodeEventError::Protocol)
        );
    }
}
