use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::text::{clean_message, has_visible_text};
use crate::{Actor, AuthenticatedPrincipal, ClientKind, Participant, ParticipantStatus, RoomEvent};

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct CommandRejection {
    pub code: &'static str,
    pub message: String,
}

impl CommandRejection {
    #[must_use]
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageSend {
    pub content: String,
}

impl MessageSend {
    /// Parses and normalizes the existing `message.send` payload.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the payload is not an object or has no visible text.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = payload
            .as_object()
            .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
        let raw = object
            .get("content")
            .or_else(|| object.get("message"))
            .and_then(Value::as_str)
            .unwrap_or_default();
        let content = clean_message(raw, 12_000);
        if !has_visible_text(&content) {
            return Err(CommandRejection::new(
                "empty",
                "Message content or an attachment is required.",
            ));
        }
        Ok(Self { content })
    }
}

#[must_use]
pub fn canonical_payload_hash(payload: &Value) -> String {
    let canonical = canonical_json(payload);
    format!("{:x}", Sha256::digest(canonical.as_bytes()))
}

/// Builds a canonical durable message event from authenticated room state.
///
/// # Errors
///
/// Returns a rejection when the principal cannot write, the participant is no
/// longer active, or the proposed room sequence is invalid.
pub fn prepare_message_event(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    command: &MessageSend,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<RoomEvent, CommandRejection> {
    if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.message_send {
        return Err(CommandRejection::new(
            "permission_denied",
            "This room session cannot send messages.",
        ));
    }
    if principal.room_id != participant.room_id
        || principal.participant_id != participant.participant_id
        || participant.status != ParticipantStatus::Joined
    {
        return Err(CommandRejection::new(
            "session_revoked",
            "This room session has ended.",
        ));
    }
    if participant.muted {
        return Err(CommandRejection::new("muted", "This participant is muted."));
    }
    if sequence <= 0 {
        return Err(CommandRejection::new(
            "invalid_state",
            "Room event sequence must be positive.",
        ));
    }
    let participant_type = participant.participant_type.clone();
    let participant_id = participant.participant_id.clone();
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: sequence,
        created_at: now,
        room_id: participant.room_id.clone(),
        event_type: "message_final".to_owned(),
        actor: Actor {
            participant_id: participant_id.clone(),
            participant_type: participant_type.clone(),
        },
        participant_id: Some(participant_id.clone()),
        participant_type: Some(participant_type.clone()),
        actor_id: Some(participant_id),
        actor_type: Some(participant_type),
        display_name: Some(participant.display_name.clone()),
        content: Some(command.content.clone()),
        message_kind: Some("message".to_owned()),
        extra: BTreeMap::new(),
    })
}

fn canonical_json(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => encode_json_string(value),
        Value::Array(values) => {
            let items = values.iter().map(canonical_json).collect::<Vec<_>>();
            format!("[{}]", items.join(","))
        }
        Value::Object(values) => {
            let entries = values
                .iter()
                .map(|(key, value)| {
                    format!("{}:{}", encode_json_string(key), canonical_json(value))
                })
                .collect::<Vec<_>>();
            format!("{{{}}}", entries.join(","))
        }
    }
}

fn encode_json_string(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() + 2);
    encoded.push('"');
    for character in value.chars() {
        match character {
            '"' => encoded.push_str("\\\""),
            '\\' => encoded.push_str("\\\\"),
            '\u{08}' => encoded.push_str("\\b"),
            '\u{0c}' => encoded.push_str("\\f"),
            '\n' => encoded.push_str("\\n"),
            '\r' => encoded.push_str("\\r"),
            '\t' => encoded.push_str("\\t"),
            character if character <= '\u{1f}' => {
                use std::fmt::Write as _;
                let _ = write!(encoded, "\\u{:04x}", u32::from(character));
            }
            character => encoded.push(character),
        }
    }
    encoded.push('"');
    encoded
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::canonical_payload_hash;

    #[test]
    fn canonical_hash_ignores_object_key_order() {
        assert_eq!(
            canonical_payload_hash(&json!({"b": 2, "a": "line\n"})),
            canonical_payload_hash(&json!({"a": "line\n", "b": 2}))
        );
    }
}
