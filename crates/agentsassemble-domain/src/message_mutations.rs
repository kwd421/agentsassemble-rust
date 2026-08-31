use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

use crate::{
    Actor, AuthenticatedPrincipal, ClientKind, CommandRejection, Participant, RoomEvent,
    clean_message, is_message_event_id,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageEdit {
    pub event_id: String,
    pub content: String,
}

impl MessageEdit {
    /// Parses one exact lobby-message edit request.
    ///
    /// # Errors
    ///
    /// Rejects aliases, extra fields, malformed event IDs, and non-string content.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = exact_object(payload, &["content", "event_id"], "message.edit")?;
        let event_id = exact_event_id(object.get("event_id"))?;
        let content = object
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                CommandRejection::new("bad_request", "message.edit content must be a string.")
            })?;
        Ok(Self {
            event_id,
            content: clean_message(content, 12_000),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageDelete {
    pub event_id: String,
}

impl MessageDelete {
    /// Parses one exact lobby-message delete request.
    ///
    /// # Errors
    ///
    /// Rejects aliases, extra fields, and malformed event IDs.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let object = exact_object(payload, &["event_id"], "message.delete")?;
        Ok(Self {
            event_id: exact_event_id(object.get("event_id"))?,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutableMessageKind {
    Message,
    Vote,
}

/// Validates the stored target shared by edit and delete.
///
/// # Errors
///
/// Rejects missing/deleted/non-message records, unsupported kinds, and inconsistent actor fields.
pub fn require_mutable_message(event: &RoomEvent) -> Result<MutableMessageKind, CommandRejection> {
    if event.event_type != "message_final" {
        return Err(CommandRejection::new(
            "message_not_found",
            "Message was not found.",
        ));
    }
    if event.extra.get("message_deleted") == Some(&Value::Bool(true)) {
        return Err(CommandRejection::new(
            "message_deleted",
            "Message was already deleted.",
        ));
    }
    let kind = match event.message_kind.as_deref() {
        Some("message") => MutableMessageKind::Message,
        Some("vote") => MutableMessageKind::Vote,
        _ => {
            return Err(CommandRejection::new(
                "unsupported_message_type",
                "This message type cannot be changed here.",
            ));
        }
    };
    require_canonical_actor(event)?;
    Ok(kind)
}

/// Authorizes an edit against current browser and target authority.
///
/// # Errors
///
/// Only the current human author of an ordinary message may edit it.
pub fn authorize_message_edit(
    principal: &AuthenticatedPrincipal,
    event: &RoomEvent,
) -> Result<(), CommandRejection> {
    require_modify_capability(principal)?;
    if require_mutable_message(event)? != MutableMessageKind::Message
        || event.actor.participant_type != "human"
        || event.actor.participant_id != principal.participant_id
    {
        return Err(CommandRejection::new(
            "permission_denied",
            "Only the author can edit this message.",
        ));
    }
    Ok(())
}

/// Authorizes deletion from current room and participant ownership.
///
/// # Errors
///
/// Only the human author, exact Agent Session owner, or current operator may delete.
pub fn authorize_message_delete(
    principal: &AuthenticatedPrincipal,
    event: &RoomEvent,
    author: &Participant,
) -> Result<MutableMessageKind, CommandRejection> {
    require_modify_capability(principal)?;
    let kind = require_mutable_message(event)?;
    if author.room_id != event.room_id
        || author.participant_id != event.actor.participant_id
        || author.participant_type != event.actor.participant_type
    {
        return Err(CommandRejection::new(
            "invalid_state",
            "Stored message author authority is inconsistent.",
        ));
    }
    let own_human =
        author.participant_type == "human" && author.participant_id == principal.participant_id;
    let owned_agent =
        author.participant_type == "agent" && author.owner_id == principal.participant_id;
    if !principal.is_operator && !own_human && !owned_agent {
        return Err(CommandRejection::new(
            "permission_denied",
            "You cannot delete this message.",
        ));
    }
    Ok(kind)
}

#[must_use]
pub fn prepare_updated_message(
    target: &RoomEvent,
    content: String,
    edited_at: DateTime<Utc>,
) -> RoomEvent {
    let mut updated = target.clone();
    updated.content = Some(content);
    updated
        .extra
        .insert("edited_at".to_owned(), json!(edited_at));
    updated
}

#[must_use]
pub fn prepare_deleted_message(
    target: &RoomEvent,
    kind: MutableMessageKind,
    deleted_at: DateTime<Utc>,
) -> RoomEvent {
    let mut deleted = target.clone();
    deleted.content = Some(String::new());
    deleted.extra.remove("target_agent_id");
    deleted.extra.insert("attachments".to_owned(), json!([]));
    deleted
        .extra
        .insert("message_deleted".to_owned(), json!(true));
    deleted
        .extra
        .insert("deleted_at".to_owned(), json!(deleted_at));
    if kind == MutableMessageKind::Vote {
        deleted.extra.insert("vote_question".to_owned(), json!(""));
        deleted.extra.insert("vote_options".to_owned(), json!([]));
        deleted
            .extra
            .insert("vote_duration_seconds".to_owned(), Value::Null);
        deleted
            .extra
            .insert("vote_deadline_at".to_owned(), json!(""));
    }
    deleted
}

#[must_use]
pub fn prepare_message_updated_event(
    principal: &AuthenticatedPrincipal,
    target: &RoomEvent,
    content: String,
    sequence: i64,
    edited_at: DateTime<Utc>,
) -> RoomEvent {
    mutation_event(
        principal,
        target,
        "message_updated",
        sequence,
        edited_at,
        Some(content),
        "edited_at",
    )
}

#[must_use]
pub fn prepare_message_deleted_event(
    principal: &AuthenticatedPrincipal,
    target: &RoomEvent,
    sequence: i64,
    deleted_at: DateTime<Utc>,
) -> RoomEvent {
    mutation_event(
        principal,
        target,
        "message_deleted",
        sequence,
        deleted_at,
        None,
        "deleted_at",
    )
}

fn mutation_event(
    principal: &AuthenticatedPrincipal,
    target: &RoomEvent,
    event_type: &str,
    sequence: i64,
    created_at: DateTime<Utc>,
    content: Option<String>,
    timestamp_key: &str,
) -> RoomEvent {
    RoomEvent {
        v: 1,
        id: uuid::Uuid::new_v4().to_string(),
        seq: sequence,
        created_at,
        room_id: target.room_id.clone(),
        event_type: event_type.to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(principal.participant_id.clone()),
        participant_type: Some("human".to_owned()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(principal.display_name.clone()),
        content,
        message_kind: None,
        extra: BTreeMap::from([
            ("target_event_id".to_owned(), json!(target.id)),
            ("target_seq".to_owned(), json!(target.seq)),
            (timestamp_key.to_owned(), json!(created_at)),
        ]),
    }
}

fn require_modify_capability(principal: &AuthenticatedPrincipal) -> Result<(), CommandRejection> {
    if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.message_modify {
        return Err(CommandRejection::new(
            "permission_denied",
            "This room session cannot modify messages.",
        ));
    }
    Ok(())
}

fn require_canonical_actor(event: &RoomEvent) -> Result<(), CommandRejection> {
    let actor = &event.actor;
    if actor.participant_id.is_empty()
        || !matches!(actor.participant_type.as_str(), "human" | "agent")
        || event.participant_id.as_deref() != Some(actor.participant_id.as_str())
        || event.participant_type.as_deref() != Some(actor.participant_type.as_str())
        || event.actor_id.as_deref() != Some(actor.participant_id.as_str())
        || event.actor_type.as_deref() != Some(actor.participant_type.as_str())
    {
        return Err(CommandRejection::new(
            "invalid_state",
            "Stored message actor authority is inconsistent.",
        ));
    }
    Ok(())
}

fn exact_object<'a>(
    payload: &'a Value,
    expected_keys: &[&str],
    action: &str,
) -> Result<&'a serde_json::Map<String, Value>, CommandRejection> {
    let object = payload
        .as_object()
        .ok_or_else(|| CommandRejection::new("bad_request", "payload must be an object."))?;
    if object.len() != expected_keys.len()
        || expected_keys.iter().any(|key| !object.contains_key(*key))
    {
        return Err(CommandRejection::new(
            "bad_request",
            format!("{action} payload has an invalid shape."),
        ));
    }
    Ok(object)
}

fn exact_event_id(value: Option<&Value>) -> Result<String, CommandRejection> {
    let event_id = value.and_then(Value::as_str).ok_or_else(|| {
        CommandRejection::new(
            "bad_request",
            "event_id must be a valid message identifier.",
        )
    })?;
    if !is_message_event_id(event_id) {
        return Err(CommandRejection::new(
            "bad_request",
            "event_id must be a valid message identifier.",
        ));
    }
    Ok(event_id.to_owned())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use chrono::Utc;
    use serde_json::json;

    use super::{
        MessageDelete, MessageEdit, MutableMessageKind, authorize_message_delete,
        authorize_message_edit, prepare_deleted_message,
    };
    use crate::{
        Actor, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant,
        ParticipantRole, ParticipantStatus, RoomEvent,
    };

    #[test]
    fn mutation_payloads_are_exact_and_edit_normalizes_content() {
        assert_eq!(
            MessageEdit::from_payload(&json!({"event_id": "event-1", "content": " x\r\n"}))
                .unwrap_or_else(|error| panic!("parse edit: {error}"))
                .content,
            "x"
        );
        assert!(MessageEdit::from_payload(&json!({"event_id": "event-1"})).is_err());
        assert!(
            MessageEdit::from_payload(
                &json!({"event_id": "event-1", "content": "x", "kind": "message"})
            )
            .is_err()
        );
        assert!(MessageDelete::from_payload(&json!({"event_id": ""})).is_err());
        assert!(
            MessageDelete::from_payload(&json!({"event_id": "event-1", "force": true})).is_err()
        );
    }

    #[test]
    fn edit_and_delete_use_exact_human_and_agent_ownership() {
        let human = participant("human-1", "human", "");
        let agent = participant("agent-1", "agent", "human-1");
        let human_principal = principal("human-1", false, InviteScope::ReadWrite);
        let message = event("human-1", "human", "message");
        let poll = event("agent-1", "agent", "vote");
        assert!(authorize_message_edit(&human_principal, &message).is_ok());
        assert_eq!(
            authorize_message_delete(&human_principal, &poll, &agent),
            Ok(MutableMessageKind::Vote)
        );
        assert_eq!(
            authorize_message_delete(&human_principal, &message, &human),
            Ok(MutableMessageKind::Message)
        );
        assert!(authorize_message_edit(&human_principal, &poll).is_err());
        assert!(
            authorize_message_delete(
                &human_principal,
                &event("agent-2", "agent", "message"),
                &participant("agent-2", "agent", "human-2")
            )
            .is_err()
        );
        let read_only = principal("human-1", false, InviteScope::ReadOnly);
        assert!(authorize_message_edit(&read_only, &message).is_err());
        let operator = principal("operator", true, InviteScope::ReadWrite);
        assert!(authorize_message_delete(&operator, &poll, &agent).is_ok());
    }

    #[test]
    fn poll_tombstone_removes_the_poll_definition() {
        let now = Utc::now();
        let mut poll = event("agent-1", "agent", "vote");
        poll.content = Some("secret question".to_owned());
        poll.extra
            .insert("attachments".to_owned(), json!([{"id": "private"}]));
        poll.extra
            .insert("vote_question".to_owned(), json!("secret question"));
        poll.extra
            .insert("vote_options".to_owned(), json!(["yes", "no"]));
        let deleted = prepare_deleted_message(&poll, MutableMessageKind::Vote, now);
        assert_eq!(deleted.extra.get("vote_options"), Some(&json!([])));
        assert_eq!(deleted.extra.get("attachments"), Some(&json!([])));
        assert_eq!(deleted.extra.get("message_deleted"), Some(&json!(true)));
    }

    fn principal(
        participant_id: &str,
        is_operator: bool,
        scope: InviteScope,
    ) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: format!("user-{participant_id}"),
            participant_id: participant_id.to_owned(),
            display_name: participant_id.to_owned(),
            room_id: "room".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: scope,
            is_operator,
            capabilities: CapabilitySet::for_principal(ClientKind::Browser, scope, is_operator),
        }
    }

    fn participant(id: &str, participant_type: &str, owner_id: &str) -> Participant {
        let now = Utc::now();
        Participant {
            room_id: "room".to_owned(),
            participant_id: id.to_owned(),
            display_name: id.to_owned(),
            avatar_image_url: String::new(),
            participant_type: participant_type.to_owned(),
            status: ParticipantStatus::Joined,
            role: if participant_type == "agent" {
                ParticipantRole::Agent
            } else {
                ParticipantRole::Human
            },
            owner_id: owner_id.to_owned(),
            muted: false,
            created_at: now,
            updated_at: now,
        }
    }

    fn event(actor_id: &str, actor_type: &str, kind: &str) -> RoomEvent {
        RoomEvent {
            v: 1,
            id: "event-1".to_owned(),
            seq: 1,
            created_at: Utc::now(),
            room_id: "room".to_owned(),
            event_type: "message_final".to_owned(),
            actor: Actor {
                participant_id: actor_id.to_owned(),
                participant_type: actor_type.to_owned(),
            },
            participant_id: Some(actor_id.to_owned()),
            participant_type: Some(actor_type.to_owned()),
            actor_id: Some(actor_id.to_owned()),
            actor_type: Some(actor_type.to_owned()),
            display_name: Some(actor_id.to_owned()),
            content: Some("content".to_owned()),
            message_kind: Some(kind.to_owned()),
            extra: BTreeMap::default(),
        }
    }
}
