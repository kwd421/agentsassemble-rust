use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use uuid::Uuid;

use crate::{CommandRejection, MAX_TEXT_CHAT_CHARACTERS, clean_message, has_visible_text};

pub const SIDE_CHAT_MAX_MESSAGES: usize = 200;
pub const SIDE_CHAT_TTL_SECONDS: i64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SideChatMessage {
    pub id: Uuid,
    pub seq: i64,
    pub created_at: DateTime<Utc>,
    pub participant_id: String,
    pub display_name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SideChatSnapshot {
    pub room_id: String,
    pub room_uid: Uuid,
    pub generation: Uuid,
    pub retained_after_seq: i64,
    pub latest_seq: i64,
    pub messages: Vec<SideChatMessage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SideChatUpdate {
    pub room_id: String,
    pub generation: Uuid,
    pub retained_after_seq: i64,
    pub message: SideChatMessage,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SideChatSend {
    pub generation: Uuid,
    pub after_seq: i64,
    pub content: String,
}

impl SideChatSend {
    /// Parses an immutable send intent bound to the observed ephemeral lifetime.
    ///
    /// # Errors
    /// Rejects unsupported fields, invalid cursors or invisible message text.
    pub fn from_payload(payload: &Value) -> Result<Self, CommandRejection> {
        let mut request: Self = serde_json::from_value(payload.clone()).map_err(|_| {
            CommandRejection::new(
                "bad_request",
                "Side chat requires generation, after_seq and content.",
            )
        })?;
        if request.after_seq < 0 {
            return Err(CommandRejection::new(
                "bad_request",
                "Side chat cursor is invalid.",
            ));
        }
        request.content = clean_message(&request.content, MAX_TEXT_CHAT_CHARACTERS);
        if !has_visible_text(&request.content) {
            return Err(CommandRejection::new(
                "empty",
                "Side chat text is required.",
            ));
        }
        Ok(request)
    }
}
