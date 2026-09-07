use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::{
    AuthenticatedPrincipal, CommandRejection, MAX_TEXT_CHAT_CHARACTERS, Participant, RoomEvent,
    clean_message, has_visible_text, room_settings::is_custom_channel_id,
};

pub const CHANNEL_HISTORY_PAGE_SIZE: i64 = 80;
pub const CHANNEL_MESSAGE_EVENT_TYPE: &str = "channel_message_final";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChannelMessageSend {
    pub channel_id: String,
    pub content: String,
}

impl ChannelMessageSend {
    /// Parses the retained text-only composer at the channel message boundary.
    ///
    /// # Errors
    /// Rejects unsupported fields, invalid channel identity or invisible content.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let mut command: Self = serde_json::from_value(payload.clone()).map_err(|_| {
            CommandRejection::new(
                "bad_request",
                "Channel messages require channel_id and content.",
            )
        })?;
        if !is_custom_channel_id(&command.channel_id) {
            return Err(CommandRejection::new(
                "bad_request",
                "Channel id is invalid.",
            ));
        }
        command.content = clean_message(&command.content, MAX_TEXT_CHAT_CHARACTERS);
        if !has_visible_text(&command.content) {
            return Err(CommandRejection::new(
                "empty",
                "Channel message text is required.",
            ));
        }
        Ok(command)
    }
}

/// Builds a channel-owned record using the existing room participant write policy.
///
/// # Errors
/// Rejects revoked/read-only/muted writers or an invalid room event sequence.
pub fn prepare_channel_message_event(
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    command: &ChannelMessageSend,
    sequence: i64,
    now: DateTime<Utc>,
) -> Result<RoomEvent, CommandRejection> {
    let mut event = crate::command::prepare_participant_message_event(
        principal,
        participant,
        sequence,
        now,
        command.content.clone(),
        "message",
        BTreeMap::from([("channel_id".to_owned(), json!(command.channel_id))]),
    )?;
    CHANNEL_MESSAGE_EVENT_TYPE.clone_into(&mut event.event_type);
    Ok(event)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ChannelHistoryPage {
    pub room_id: String,
    pub channel_id: String,
    pub events: Vec<RoomEvent>,
    pub oldest_seq: i64,
    pub last_seq: i64,
    pub has_more_before: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_channel_payload_is_bounded_and_cannot_borrow_attachment_or_lobby_identity() {
        let parsed = ChannelMessageSend::from_payload(&json!({
            "channel_id": "c0123456789ab", "content": "한".repeat(2001)
        }))
        .unwrap_or_else(|error| panic!("channel payload: {error}"));
        assert_eq!(parsed.content.chars().count(), MAX_TEXT_CHAT_CHARACTERS);
        for payload in [
            json!({"channel_id": "lobby", "content": "text"}),
            json!({"channel_id": "c0123456789ab", "content": "text", "attachment_ids": []}),
            json!({"channel_id": "c0123456789ab", "content": "  "}),
        ] {
            assert!(ChannelMessageSend::from_payload(&payload).is_err());
        }
    }
}
