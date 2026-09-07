use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

use crate::text::{MAX_MESSAGE_CHARACTERS, clean_message, has_visible_text};
use crate::{
    Actor, AuthenticatedPrincipal, ClientKind, MAX_MESSAGE_ATTACHMENTS_PER_EVENT, Participant,
    ParticipantStatus, RoomEvent, is_message_attachment_id,
};

/// Accepted identity keys for an Agent Session lifecycle command. Exactly one is required.
pub const AGENT_CONTROL_ID_KEYS: [&str; 3] = ["agent_id", "participant_id", "session_id"];

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
    pub attachment_ids: Vec<String>,
}

impl MessageSend {
    /// Parses and normalizes the canonical lobby `message.send` payload.
    ///
    /// # Errors
    ///
    /// Returns a rejection when the payload shape, content, or attachment identifiers are invalid.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = payload
            .as_object()
            .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
        if !object.contains_key("content")
            || object
                .keys()
                .any(|key| !matches!(key.as_str(), "content" | "attachment_ids"))
        {
            return Err(CommandRejection::new(
                "bad_request",
                "message.send accepts exactly content and optional attachment_ids fields.",
            ));
        }
        let raw = object["content"].as_str().ok_or_else(|| {
            CommandRejection::new("bad_request", "message.send content must be a string.")
        })?;
        let content = clean_message(raw, MAX_MESSAGE_CHARACTERS);
        let attachment_ids = parse_attachment_ids(object.get("attachment_ids"))?;
        if !has_visible_text(&content) && attachment_ids.is_empty() {
            return Err(CommandRejection::new(
                "empty",
                "Message content or an attachment is required.",
            ));
        }
        Ok(Self {
            content,
            attachment_ids,
        })
    }
}

pub(crate) fn parse_attachment_ids(value: Option<&Value>) -> Result<Vec<String>, CommandRejection> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let values = value
        .as_array()
        .ok_or_else(|| CommandRejection::new("bad_request", "attachment_ids must be an array."))?;
    if values.len() > MAX_MESSAGE_ATTACHMENTS_PER_EVENT {
        return Err(CommandRejection::new(
            "bad_request",
            format!("At most {MAX_MESSAGE_ATTACHMENTS_PER_EVENT} attachments are allowed."),
        ));
    }
    let mut seen = BTreeSet::new();
    let mut attachment_ids = Vec::with_capacity(values.len());
    for value in values {
        let attachment_id = value.as_str().ok_or_else(|| {
            CommandRejection::new("bad_request", "attachment_ids entries must be strings.")
        })?;
        if !is_message_attachment_id(attachment_id) {
            return Err(CommandRejection::new(
                "bad_request",
                "Message attachment id is invalid.",
            ));
        }
        if !seen.insert(attachment_id) {
            return Err(CommandRejection::new(
                "bad_request",
                "Message attachment ids must be distinct.",
            ));
        }
        attachment_ids.push(attachment_id.to_owned());
    }
    Ok(attachment_ids)
}

#[must_use]
pub fn canonical_payload_hash(payload: &Value) -> String {
    let canonical = crate::canonical_json::encode(payload);
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
    prepare_participant_message_event(
        principal,
        participant,
        sequence,
        now,
        command.content.clone(),
        "message",
        BTreeMap::new(),
    )
}

pub(crate) fn prepare_participant_message_event(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    sequence: i64,
    now: DateTime<Utc>,
    content: String,
    message_kind: &str,
    extra: BTreeMap<String, Value>,
) -> Result<RoomEvent, CommandRejection> {
    require_message_write_authority(principal, participant)?;
    prepare_authorized_participant_message_event(
        participant,
        sequence,
        now,
        content,
        message_kind,
        extra,
    )
}

pub(crate) fn prepare_authorized_participant_message_event(
    participant: &Participant,
    sequence: i64,
    now: DateTime<Utc>,
    content: String,
    message_kind: &str,
    extra: BTreeMap<String, Value>,
) -> Result<RoomEvent, CommandRejection> {
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
        content: Some(content),
        message_kind: Some(message_kind.to_owned()),
        extra,
    })
}

/// Applies the room-owned write policy shared by messages and their pending uploads.
///
/// # Errors
///
/// Returns a rejection for non-human clients, read-only authority, stale membership,
/// or a currently muted participant.
pub fn require_message_write_authority(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
) -> Result<(), CommandRejection> {
    if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.message_send {
        return Err(CommandRejection::new(
            "permission_denied",
            "This room session cannot send messages.",
        ));
    }
    require_active_write_participant(principal, participant)
}

pub(crate) fn require_active_write_participant(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
) -> Result<(), CommandRejection> {
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MessageSend, canonical_payload_hash};

    #[test]
    fn canonical_hash_ignores_object_key_order() {
        assert_eq!(
            canonical_payload_hash(&json!({"b": 2, "a": "line\n"})),
            canonical_payload_hash(&json!({"a": "line\n", "b": 2}))
        );
    }

    #[test]
    fn message_send_rejects_aliases_and_extra_fields() {
        assert!(MessageSend::from_payload(&json!({"message": "alias"})).is_err());
        assert!(
            MessageSend::from_payload(&json!({"content": "hello", "kind": "message"})).is_err()
        );
        assert_eq!(
            MessageSend::from_payload(&json!({"content": "hello"}))
                .unwrap_or_else(|error| panic!("canonical message: {error}"))
                .content,
            "hello"
        );
    }

    #[test]
    fn message_send_accepts_attachment_only_and_rejects_noncanonical_ids() {
        let first = "ma_0123456789abcdef0123456789abcdef";
        let second = "ma_fedcba9876543210fedcba9876543210";
        let command = MessageSend::from_payload(&json!({
            "content": "  ",
            "attachment_ids": [first, second]
        }))
        .unwrap_or_else(|error| panic!("attachment-only message: {error}"));
        assert_eq!(command.content, "");
        assert_eq!(command.attachment_ids, [first, second]);

        for payload in [
            json!({"content": "", "attachment_ids": []}),
            json!({"content": "ok", "attachment_ids": [first, first]}),
            json!({"content": "ok", "attachment_ids": ["ma_invalid"]}),
            json!({"content": "ok", "attachment_ids": [first], "attachments": []}),
            json!({"content": "ok", "attachment_ids": [
                first, second, first, second, first, second, first, second, first
            ]}),
        ] {
            assert!(
                MessageSend::from_payload(&payload).is_err(),
                "accepted {payload}"
            );
        }
    }
}
