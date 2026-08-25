use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;

pub const MAX_PREFERENCE_CHANNELS: usize = 54;
pub const READ_CURSOR_LIMIT: usize = 64;

const BUILTIN_CHANNEL_IDS: [&str; 4] = ["lobby", "live", "board", "records"];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoomNotificationMode {
    All,
    #[default]
    Mentions,
    Mute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelNotificationMode {
    Default,
    All,
    Mentions,
    Mute,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChannelPreference {
    pub notifications: ChannelNotificationMode,
    pub last_read_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoomUserPreferences {
    pub notifications: RoomNotificationMode,
    pub channel_settings: BTreeMap<String, ChannelPreference>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomUserPreferencesPatch {
    pub notifications: Option<RoomNotificationMode>,
    pub channel_settings: Option<BTreeMap<String, ChannelPreference>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RoomPreferencesError {
    pub code: &'static str,
    pub message: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawChannelPreference {
    notifications: ChannelNotificationMode,
    last_read_at: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoomUserPreferences {
    notifications: RoomNotificationMode,
    channel_settings: BTreeMap<String, RawChannelPreference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoomUserPreferencesPatch {
    #[serde(default, deserialize_with = "deserialize_present")]
    notifications: Option<RoomNotificationMode>,
    #[serde(default, deserialize_with = "deserialize_present")]
    channel_settings: Option<BTreeMap<String, RawChannelPreference>>,
}

impl Default for RoomUserPreferences {
    fn default() -> Self {
        Self {
            notifications: RoomNotificationMode::Mentions,
            channel_settings: BTreeMap::new(),
        }
    }
}

impl RoomUserPreferences {
    /// Applies a strict top-level partial update.
    ///
    /// A supplied `channel_settings` field replaces the complete map rather than
    /// merging individual channel entries.
    #[must_use]
    pub fn apply_patch(&self, patch: RoomUserPreferencesPatch) -> Self {
        Self {
            notifications: patch.notifications.unwrap_or(self.notifications),
            channel_settings: patch
                .channel_settings
                .unwrap_or_else(|| self.channel_settings.clone()),
        }
    }
}

impl<'de> Deserialize<'de> for ChannelPreference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawChannelPreference::deserialize(deserializer)?;
        canonical_channel_preference(raw).map_err(de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RoomUserPreferences {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRoomUserPreferences::deserialize(deserializer)?;
        Ok(Self {
            notifications: raw.notifications,
            channel_settings: canonical_channel_settings(raw.channel_settings)
                .map_err(de::Error::custom)?,
        })
    }
}

impl<'de> Deserialize<'de> for RoomUserPreferencesPatch {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRoomUserPreferencesPatch::deserialize(deserializer)?;
        Ok(Self {
            notifications: raw.notifications,
            channel_settings: raw
                .channel_settings
                .map(canonical_channel_settings)
                .transpose()
                .map_err(de::Error::custom)?,
        })
    }
}

fn deserialize_present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

fn canonical_channel_settings(
    raw: BTreeMap<String, RawChannelPreference>,
) -> Result<BTreeMap<String, ChannelPreference>, RoomPreferencesError> {
    if raw.len() > MAX_PREFERENCE_CHANNELS {
        return Err(invalid(format!(
            "channel_settings cannot contain more than {MAX_PREFERENCE_CHANNELS} entries."
        )));
    }
    raw.into_iter()
        .map(|(channel_id, preference)| {
            if !supported_channel_id(&channel_id) {
                return Err(invalid(format!(
                    "Unsupported preference channel id: {channel_id}."
                )));
            }
            Ok((channel_id, canonical_channel_preference(preference)?))
        })
        .collect()
}

fn canonical_channel_preference(
    raw: RawChannelPreference,
) -> Result<ChannelPreference, RoomPreferencesError> {
    if raw.last_read_at.chars().count() > READ_CURSOR_LIMIT
        || raw
            .last_read_at
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '\t'))
    {
        return Err(invalid("Read cursor is not canonical."));
    }
    Ok(ChannelPreference {
        notifications: raw.notifications,
        last_read_at: raw.last_read_at,
    })
}

fn supported_channel_id(value: &str) -> bool {
    BUILTIN_CHANNEL_IDS.contains(&value)
        || value.len() == 13
            && value.starts_with('c')
            && value[1..]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn invalid(message: impl Into<String>) -> RoomPreferencesError {
    RoomPreferencesError {
        code: "room_preferences_invalid",
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{
        ChannelNotificationMode, RoomNotificationMode, RoomUserPreferences,
        RoomUserPreferencesPatch,
    };

    #[test]
    fn complete_record_is_exact_and_uses_the_original_defaults() {
        assert_eq!(
            serde_json::to_value(RoomUserPreferences::default())
                .unwrap_or_else(|error| panic!("serialize defaults: {error}")),
            json!({"notifications": "mentions", "channel_settings": {}})
        );
        assert!(
            serde_json::from_value::<RoomUserPreferences>(
                json!({"notifications": "mentions", "channel_settings": {}, "extra": true})
            )
            .is_err()
        );
        assert!(
            serde_json::from_value::<RoomUserPreferences>(json!({"notifications": "mentions"}))
                .is_err()
        );
    }

    #[test]
    fn cursor_preserves_unicode_and_whitespace_without_normalization() {
        let cursor = format!("  {}\u{0000}", "한".repeat(61));
        assert_eq!(cursor.chars().count(), 64);
        let preferences: RoomUserPreferences = serde_json::from_value(json!({
            "notifications": "all",
            "channel_settings": {
                "lobby": {"notifications": "default", "last_read_at": cursor}
            }
        }))
        .unwrap_or_else(|error| panic!("parse exact cursor: {error}"));
        assert_eq!(preferences.channel_settings["lobby"].last_read_at, cursor);

        for rejected in [
            "한".repeat(65),
            "line\nfeed".to_owned(),
            "tab\there".to_owned(),
        ] {
            assert!(
                serde_json::from_value::<RoomUserPreferences>(json!({
                    "notifications": "mentions",
                    "channel_settings": {
                        "lobby": {"notifications": "all", "last_read_at": rejected}
                    }
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn channel_ids_and_total_count_are_strict() {
        let mut channel_settings = serde_json::Map::new();
        for builtin in ["lobby", "live", "board", "records"] {
            channel_settings.insert(
                builtin.to_owned(),
                json!({"notifications": "default", "last_read_at": ""}),
            );
        }
        for index in 0..50 {
            channel_settings.insert(
                format!("c{index:012x}"),
                json!({"notifications": "mentions", "last_read_at": "cursor"}),
            );
        }
        assert!(
            serde_json::from_value::<RoomUserPreferences>(json!({
                "notifications": "mute",
                "channel_settings": channel_settings,
            }))
            .is_ok()
        );

        channel_settings.insert(
            "cffffffffffff".to_owned(),
            json!({"notifications": "all", "last_read_at": ""}),
        );
        assert!(
            serde_json::from_value::<RoomUserPreferences>(json!({
                "notifications": "mute",
                "channel_settings": channel_settings,
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<RoomUserPreferences>(json!({
                "notifications": "mentions",
                "channel_settings": {
                    "cABCDEF123456": {"notifications": "all", "last_read_at": ""}
                }
            }))
            .is_err()
        );
    }

    #[test]
    fn partial_update_replaces_the_complete_channel_map() {
        let current: RoomUserPreferences = serde_json::from_value(json!({
            "notifications": "mentions",
            "channel_settings": {
                "lobby": {"notifications": "mute", "last_read_at": "old"}
            }
        }))
        .unwrap_or_else(|error| panic!("parse current preferences: {error}"));
        let patch: RoomUserPreferencesPatch = serde_json::from_value(json!({
            "channel_settings": {
                "records": {"notifications": "all", "last_read_at": "new"}
            }
        }))
        .unwrap_or_else(|error| panic!("parse preference patch: {error}"));
        let updated = current.apply_patch(patch);
        assert_eq!(updated.notifications, RoomNotificationMode::Mentions);
        assert_eq!(updated.channel_settings.len(), 1);
        assert_eq!(
            updated.channel_settings["records"].notifications,
            ChannelNotificationMode::All
        );
        assert!(!updated.channel_settings.contains_key("lobby"));

        for invalid in [
            json!({"notifications": null}),
            json!({"unknown": Value::Null}),
        ] {
            assert!(serde_json::from_value::<RoomUserPreferencesPatch>(invalid).is_err());
        }
    }
}
