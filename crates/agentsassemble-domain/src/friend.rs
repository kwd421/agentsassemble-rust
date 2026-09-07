use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FriendParticipantType {
    Human,
    SubscriptionAi,
    Api,
    Local,
    Remote,
    Unknown,
}

/// Saved contact metadata never grants admission or establishes live presence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct FriendDetails {
    pub display_name: String,
    pub handle: String,
    pub participant_type: FriendParticipantType,
    pub provider_kind: String,
    pub connection_kind: String,
    pub agent_id: String,
    pub source_agent_id: String,
    pub last_meeting_id: String,
    pub status: String,
    pub source: String,
    pub last_seen_at: Option<DateTime<Utc>>,
}

impl FriendDetails {
    #[must_use]
    pub fn is_valid(&self) -> bool {
        !self.display_name.trim().is_empty()
            && [&self.display_name, &self.handle]
                .into_iter()
                .all(|text| valid_text(text, 120))
            && [
                &self.provider_kind,
                &self.connection_kind,
                &self.agent_id,
                &self.source_agent_id,
                &self.last_meeting_id,
                &self.status,
                &self.source,
            ]
            .into_iter()
            .all(|text| valid_text(text, 256))
    }
}

fn valid_text(text: &str, limit: usize) -> bool {
    text.chars().count() <= limit && !text.chars().any(char::is_control)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SavedFriend {
    pub friend_id: Uuid,
    pub revision: i64,
    pub details: FriendDetails,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Revision zero creates a contact; a positive revision edits the observed version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SaveFriend {
    pub friend_id: Uuid,
    pub expected_revision: i64,
    pub details: FriendDetails,
}
