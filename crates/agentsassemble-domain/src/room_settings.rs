use std::{collections::BTreeSet, sync::LazyLock};

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;
use ts_rs::TS;

use crate::clean_single_line;

const ROOM_LABEL_LIMIT: usize = 128;
const ROOM_TOPIC_LIMIT: usize = 160;
const IMAGE_URL_LIMIT: usize = 240;
const CHANNEL_NAME_LIMIT: usize = 60;
const MAX_CHANNELS: usize = 50;
pub const ROOM_APPEARANCE_ASSET_PREFIX: &str = "ra_";
pub const ROOM_APPEARANCE_ASSET_HEX_LENGTH: usize = 32;
pub const ROOM_APPEARANCE_REFERENCE_PREFIX: &str = "/api/attachments/";
pub const ROOM_APPEARANCE_REFERENCE_SUFFIX: &str = "?view=1";

static CHANNEL_ID: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^c[0-9a-f]{12}$").unwrap_or_else(|error| panic!("valid channel regex: {error}"))
});
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct RoomAppearance {
    pub banner_preset: String,
    pub banner_image_url: String,
    pub icon_image_url: String,
    pub icon_label: String,
    pub invite_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RoomChannel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub channel_type: String,
    pub position: u32,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
pub struct RoomSettings {
    pub label: String,
    pub topic: String,
    pub appearance: RoomAppearance,
    pub conversation_mode: String,
    pub tool_mode: String,
    pub ordered_exclude_previous_speaker: bool,
    pub channels: Vec<RoomChannel>,
    pub activity_plugin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct PublicRoomSettings {
    pub settings_revision: String,
    pub label: String,
    pub topic: String,
    pub appearance: RoomAppearance,
    pub conversation_mode: String,
    pub tool_mode: String,
    pub ordered_exclude_previous_speaker: bool,
    pub channels: Vec<RoomChannel>,
    pub activity_plugin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoomSettingsPatch {
    pub label: Option<String>,
    pub topic: Option<String>,
    pub appearance: RoomAppearancePatch,
    pub conversation_mode: Option<String>,
    pub tool_mode: Option<String>,
    pub ordered_exclude_previous_speaker: Option<bool>,
    pub channels: Option<Vec<RoomChannel>>,
    pub activity_plugin: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RoomAppearancePatch {
    pub banner_preset: Option<String>,
    pub banner_image_url: Option<String>,
    pub icon_image_url: Option<String>,
    pub icon_label: Option<String>,
    pub invite_scope: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{message}")]
pub struct RoomSettingsError {
    pub code: &'static str,
    pub message: String,
}

impl RoomSettingsError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            code: "bad_request",
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            code: "room_setting_unsupported",
            message: message.into(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoomAppearance {
    banner_preset: String,
    banner_image_url: String,
    icon_image_url: String,
    icon_label: String,
    invite_scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawRoomSettings {
    label: String,
    topic: String,
    appearance: RawRoomAppearance,
    conversation_mode: String,
    tool_mode: String,
    ordered_exclude_previous_speaker: bool,
    channels: Vec<RoomChannel>,
    activity_plugin: String,
}

impl<'de> Deserialize<'de> for RoomAppearance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRoomAppearance::deserialize(deserializer)?;
        validate_appearance(raw).map_err(serde::de::Error::custom)
    }
}

impl<'de> Deserialize<'de> for RoomSettings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawRoomSettings::deserialize(deserializer)?;
        validate_settings(raw).map_err(serde::de::Error::custom)
    }
}

impl RoomSettings {
    #[must_use]
    pub fn defaults(label: &str) -> Self {
        Self {
            label: clean_single_line(label, ROOM_LABEL_LIMIT),
            topic: String::new(),
            appearance: RoomAppearance {
                banner_preset: "default".to_owned(),
                banner_image_url: String::new(),
                icon_image_url: String::new(),
                icon_label: String::new(),
                invite_scope: "room".to_owned(),
            },
            conversation_mode: "ordered".to_owned(),
            tool_mode: "chat".to_owned(),
            ordered_exclude_previous_speaker: true,
            channels: Vec::new(),
            activity_plugin: String::new(),
        }
    }

    /// Parses one strict partial WebSocket mutation and its required revision.
    ///
    /// # Errors
    ///
    /// Rejects unknown, noncanonical, unavailable, empty, or revisionless updates.
    pub fn strict_update(&self, payload: &Value) -> Result<(String, Self), RoomSettingsError> {
        let object = payload
            .as_object()
            .ok_or_else(|| RoomSettingsError::bad_request("payload must be an object."))?;
        require_known_keys(
            object,
            &[
                "expected_revision",
                "label",
                "topic",
                "appearance",
                "conversation_mode",
                "tool_mode",
                "ordered_exclude_previous_speaker",
                "channels",
                "activity_plugin",
            ],
            "room settings",
        )?;
        let expected_revision = required_string(object, "expected_revision")?;
        if expected_revision.chars().count() > 96
            || clean_single_line(&expected_revision, 96) != expected_revision
        {
            return Err(RoomSettingsError::bad_request(
                "expected_revision is not canonical.",
            ));
        }
        let patch = RoomSettingsPatch::parse(object)?;
        if patch.is_empty() {
            return Err(RoomSettingsError::bad_request(
                "At least one room-global setting is required.",
            ));
        }
        patch.require_available()?;
        let next = patch.apply(self)?;
        Ok((expected_revision, next))
    }
}

impl RoomSettingsPatch {
    fn parse(object: &Map<String, Value>) -> Result<Self, RoomSettingsError> {
        Ok(Self {
            label: optional_string(object, "label")?,
            topic: optional_string(object, "topic")?,
            appearance: object.get("appearance").map_or_else(
                || Ok(RoomAppearancePatch::default()),
                RoomAppearancePatch::parse,
            )?,
            conversation_mode: optional_string(object, "conversation_mode")?,
            tool_mode: optional_string(object, "tool_mode")?,
            ordered_exclude_previous_speaker: optional_bool(
                object,
                "ordered_exclude_previous_speaker",
            )?,
            channels: optional_typed(object, "channels")?,
            activity_plugin: optional_string(object, "activity_plugin")?,
        })
    }

    fn is_empty(&self) -> bool {
        self.label.is_none()
            && self.topic.is_none()
            && self.appearance.is_empty()
            && self.conversation_mode.is_none()
            && self.tool_mode.is_none()
            && self.ordered_exclude_previous_speaker.is_none()
            && self.channels.is_none()
            && self.activity_plugin.is_none()
    }

    fn require_available(&self) -> Result<(), RoomSettingsError> {
        if self.channels.is_some() {
            return Err(RoomSettingsError::unsupported(
                "Custom channels are unavailable until their message and voice owners exist.",
            ));
        }
        if self.activity_plugin.is_some() {
            return Err(RoomSettingsError::unsupported(
                "Room activity plugins are unavailable.",
            ));
        }
        Ok(())
    }

    fn apply(&self, current: &RoomSettings) -> Result<RoomSettings, RoomSettingsError> {
        let next = RoomSettings {
            label: self.label.clone().unwrap_or_else(|| current.label.clone()),
            topic: self.topic.clone().unwrap_or_else(|| current.topic.clone()),
            appearance: self.appearance.apply(&current.appearance),
            conversation_mode: self
                .conversation_mode
                .clone()
                .unwrap_or_else(|| current.conversation_mode.clone()),
            tool_mode: self
                .tool_mode
                .clone()
                .unwrap_or_else(|| current.tool_mode.clone()),
            ordered_exclude_previous_speaker: self
                .ordered_exclude_previous_speaker
                .unwrap_or(current.ordered_exclude_previous_speaker),
            channels: self
                .channels
                .clone()
                .unwrap_or_else(|| current.channels.clone()),
            activity_plugin: self
                .activity_plugin
                .clone()
                .unwrap_or_else(|| current.activity_plugin.clone()),
        };
        validate_settings(RawRoomSettings::from(next))
    }
}

impl RoomAppearancePatch {
    fn parse(value: &Value) -> Result<Self, RoomSettingsError> {
        let object = value
            .as_object()
            .ok_or_else(|| RoomSettingsError::bad_request("appearance must be an object."))?;
        require_known_keys(
            object,
            &[
                "banner_preset",
                "banner_image_url",
                "icon_image_url",
                "icon_label",
                "invite_scope",
            ],
            "appearance",
        )?;
        if object.is_empty() {
            return Err(RoomSettingsError::bad_request(
                "appearance update must not be empty.",
            ));
        }
        Ok(Self {
            banner_preset: optional_string(object, "banner_preset")?,
            banner_image_url: optional_string(object, "banner_image_url")?,
            icon_image_url: optional_string(object, "icon_image_url")?,
            icon_label: optional_string(object, "icon_label")?,
            invite_scope: optional_string(object, "invite_scope")?,
        })
    }

    fn is_empty(&self) -> bool {
        self.banner_preset.is_none()
            && self.banner_image_url.is_none()
            && self.icon_image_url.is_none()
            && self.icon_label.is_none()
            && self.invite_scope.is_none()
    }

    fn apply(&self, current: &RoomAppearance) -> RoomAppearance {
        RoomAppearance {
            banner_preset: self
                .banner_preset
                .clone()
                .unwrap_or_else(|| current.banner_preset.clone()),
            banner_image_url: self
                .banner_image_url
                .clone()
                .unwrap_or_else(|| current.banner_image_url.clone()),
            icon_image_url: self
                .icon_image_url
                .clone()
                .unwrap_or_else(|| current.icon_image_url.clone()),
            icon_label: self
                .icon_label
                .clone()
                .unwrap_or_else(|| current.icon_label.clone()),
            invite_scope: self
                .invite_scope
                .clone()
                .unwrap_or_else(|| current.invite_scope.clone()),
        }
    }
}

impl From<RoomSettings> for RawRoomSettings {
    fn from(value: RoomSettings) -> Self {
        Self {
            label: value.label,
            topic: value.topic,
            appearance: RawRoomAppearance {
                banner_preset: value.appearance.banner_preset,
                banner_image_url: value.appearance.banner_image_url,
                icon_image_url: value.appearance.icon_image_url,
                icon_label: value.appearance.icon_label,
                invite_scope: value.appearance.invite_scope,
            },
            conversation_mode: value.conversation_mode,
            tool_mode: value.tool_mode,
            ordered_exclude_previous_speaker: value.ordered_exclude_previous_speaker,
            channels: value.channels,
            activity_plugin: value.activity_plugin,
        }
    }
}

fn validate_settings(raw: RawRoomSettings) -> Result<RoomSettings, RoomSettingsError> {
    require_canonical_text(&raw.label, "label", ROOM_LABEL_LIMIT)?;
    require_canonical_text(&raw.topic, "topic", ROOM_TOPIC_LIMIT)?;
    if !matches!(raw.conversation_mode.as_str(), "ordered" | "ambient") {
        return Err(RoomSettingsError::bad_request(format!(
            "Unsupported conversation_mode: {}.",
            raw.conversation_mode
        )));
    }
    if !matches!(raw.tool_mode.as_str(), "chat" | "tabletop") {
        return Err(RoomSettingsError::bad_request(format!(
            "Unsupported tool_mode: {}.",
            raw.tool_mode
        )));
    }
    if raw.channels.len() > MAX_CHANNELS {
        return Err(RoomSettingsError::bad_request(format!(
            "channels cannot contain more than {MAX_CHANNELS} entries."
        )));
    }
    let mut seen = BTreeSet::new();
    for (position, channel) in raw.channels.iter().enumerate() {
        if !CHANNEL_ID.is_match(&channel.id) || !seen.insert(channel.id.as_str()) {
            return Err(RoomSettingsError::bad_request(
                "Channel ids must be unique canonical ids.",
            ));
        }
        require_canonical_text(&channel.name, "channel name", CHANNEL_NAME_LIMIT)?;
        if channel.name.is_empty() || !matches!(channel.channel_type.as_str(), "text" | "voice") {
            return Err(RoomSettingsError::bad_request(
                "Channel name or type is invalid.",
            ));
        }
        if usize::try_from(channel.position).ok() != Some(position) {
            return Err(RoomSettingsError::bad_request(
                "Channel positions must be dense and match list order.",
            ));
        }
        require_canonical_text(&channel.created_at, "channel created_at", 64)?;
    }
    if !raw.activity_plugin.is_empty() {
        require_canonical_text(&raw.activity_plugin, "activity_plugin", 64)?;
        if raw.activity_plugin.to_lowercase() != raw.activity_plugin {
            return Err(RoomSettingsError::bad_request(
                "activity_plugin must be a canonical lowercase id.",
            ));
        }
    }
    Ok(RoomSettings {
        label: raw.label,
        topic: raw.topic,
        appearance: validate_appearance(raw.appearance)?,
        conversation_mode: raw.conversation_mode,
        tool_mode: raw.tool_mode,
        ordered_exclude_previous_speaker: raw.ordered_exclude_previous_speaker,
        channels: raw.channels,
        activity_plugin: raw.activity_plugin,
    })
}

fn validate_appearance(raw: RawRoomAppearance) -> Result<RoomAppearance, RoomSettingsError> {
    require_canonical_text(&raw.banner_preset, "banner_preset", 24)?;
    if !matches!(
        raw.banner_preset.as_str(),
        "default" | "forest" | "midnight" | "ember" | "custom"
    ) {
        return Err(RoomSettingsError::bad_request(format!(
            "Unsupported banner_preset: {}.",
            raw.banner_preset
        )));
    }
    require_asset_url(&raw.banner_image_url, "banner_image_url")?;
    require_asset_url(&raw.icon_image_url, "icon_image_url")?;
    require_short_label(&raw.icon_label)?;
    if !matches!(raw.invite_scope.as_str(), "room" | "read_only") {
        return Err(RoomSettingsError::bad_request(format!(
            "Unsupported invite_scope: {}.",
            raw.invite_scope
        )));
    }
    Ok(RoomAppearance {
        banner_preset: raw.banner_preset,
        banner_image_url: raw.banner_image_url,
        icon_image_url: raw.icon_image_url,
        icon_label: raw.icon_label,
        invite_scope: raw.invite_scope,
    })
}

fn require_canonical_text(value: &str, field: &str, limit: usize) -> Result<(), RoomSettingsError> {
    if clean_single_line(value, limit) != value {
        return Err(RoomSettingsError::bad_request(format!(
            "{field} must be canonical single-line text up to {limit} characters."
        )));
    }
    Ok(())
}

fn require_asset_url(value: &str, field: &str) -> Result<(), RoomSettingsError> {
    require_canonical_text(value, field, IMAGE_URL_LIMIT)?;
    if !value.is_empty() && room_appearance_asset_id(value).is_none() {
        return Err(RoomSettingsError::bad_request(format!(
            "{field} must be empty or a canonical room attachment URL."
        )));
    }
    Ok(())
}

/// Returns the opaque room-owned asset ID from one exact renderable appearance URL.
#[must_use]
pub fn room_appearance_asset_id(value: &str) -> Option<&str> {
    let asset_id = value
        .strip_prefix(ROOM_APPEARANCE_REFERENCE_PREFIX)?
        .strip_suffix(ROOM_APPEARANCE_REFERENCE_SUFFIX)?;
    is_room_appearance_asset_id(asset_id).then_some(asset_id)
}

/// Reports whether one opaque identifier belongs to the room-appearance namespace.
#[must_use]
pub fn is_room_appearance_asset_id(asset_id: &str) -> bool {
    let Some(hex) = asset_id.strip_prefix(ROOM_APPEARANCE_ASSET_PREFIX) else {
        return false;
    };
    hex.len() == ROOM_APPEARANCE_ASSET_HEX_LENGTH
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn require_short_label(value: &str) -> Result<(), RoomSettingsError> {
    let clean = clean_single_line(value, 2)
        .chars()
        .flat_map(char::to_uppercase)
        .take(2)
        .collect::<String>();
    if clean != value {
        return Err(RoomSettingsError::bad_request(
            "icon_label is not canonical.",
        ));
    }
    Ok(())
}

fn require_known_keys(
    object: &Map<String, Value>,
    keys: &[&str],
    field: &str,
) -> Result<(), RoomSettingsError> {
    let unknown = object
        .keys()
        .filter(|key| !keys.contains(&key.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if unknown.is_empty() {
        Ok(())
    } else {
        Err(RoomSettingsError::bad_request(format!(
            "Unsupported {field} fields: {}.",
            unknown.join(", ")
        )))
    }
}

fn required_string(object: &Map<String, Value>, field: &str) -> Result<String, RoomSettingsError> {
    object
        .get(field)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| RoomSettingsError::bad_request(format!("{field} is required.")))
}

fn optional_string(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<String>, RoomSettingsError> {
    object.get(field).map_or(Ok(None), |value| {
        value
            .as_str()
            .map(|value| Some(value.to_owned()))
            .ok_or_else(|| RoomSettingsError::bad_request(format!("{field} must be a string.")))
    })
}

fn optional_bool(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<bool>, RoomSettingsError> {
    object.get(field).map_or(Ok(None), |value| {
        value
            .as_bool()
            .map(Some)
            .ok_or_else(|| RoomSettingsError::bad_request(format!("{field} must be a boolean.")))
    })
}

fn optional_typed<T: for<'de> Deserialize<'de>>(
    object: &Map<String, Value>,
    field: &str,
) -> Result<Option<T>, RoomSettingsError> {
    object.get(field).map_or(Ok(None), |value| {
        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|_| RoomSettingsError::bad_request(format!("{field} is invalid.")))
    })
}

/// Serializes room settings and attaches their canonical public revision.
///
/// # Errors
///
/// Returns the serialization error if a settings field cannot be encoded.
pub fn public_settings(settings: &RoomSettings) -> Result<PublicRoomSettings, serde_json::Error> {
    let canonical = serde_json::to_vec(&serde_json::to_value(settings)?)?;
    let revision = format!("room-settings-v1-{:x}", Sha256::digest(canonical));
    Ok(PublicRoomSettings {
        settings_revision: revision,
        label: settings.label.clone(),
        topic: settings.topic.clone(),
        appearance: settings.appearance.clone(),
        conversation_mode: settings.conversation_mode.clone(),
        tool_mode: settings.tool_mode.clone(),
        ordered_exclude_previous_speaker: settings.ordered_exclude_previous_speaker,
        channels: settings.channels.clone(),
        activity_plugin: settings.activity_plugin.clone(),
    })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RoomSettings, public_settings, room_appearance_asset_id};

    #[test]
    fn room_settings_revision_matches_current_sorted_json_contract() {
        let settings = RoomSettings::defaults("General");
        let public = public_settings(&settings)
            .unwrap_or_else(|error| panic!("serialize canonical settings: {error}"));
        assert_eq!(
            public.settings_revision,
            "room-settings-v1-3b84a7fa08f6ced85a21f168cc03d6363751337cee7ecc0e0c31aaecc5a22b98"
        );
    }

    #[test]
    fn strict_update_rejects_unimplemented_fields() {
        let current = RoomSettings::defaults("General");
        let revision = public_settings(&current)
            .unwrap_or_else(|error| panic!("settings revision: {error}"))
            .settings_revision;
        let Err(error) = current.strict_update(&json!({
            "expected_revision": revision,
            "channels": []
        })) else {
            panic!("channels unexpectedly became available");
        };
        assert_eq!(error.code, "room_setting_unsupported");
    }

    #[test]
    fn room_invite_scope_is_mutable_after_admission_activation() {
        let current = RoomSettings::defaults("General");
        let revision = public_settings(&current)
            .unwrap_or_else(|error| panic!("settings revision: {error}"))
            .settings_revision;
        let (_, next) = current
            .strict_update(&json!({
                "expected_revision": revision,
                "appearance": {"invite_scope": "read_only"}
            }))
            .unwrap_or_else(|error| panic!("update invite scope: {error}"));

        assert_eq!(next.appearance.invite_scope, "read_only");
    }

    #[test]
    fn room_appearance_urls_reserve_the_exact_room_asset_namespace() {
        let asset_id = "ra_0123456789abcdef0123456789abcdef";
        let url = format!("/api/attachments/{asset_id}?view=1");
        assert_eq!(room_appearance_asset_id(&url), Some(asset_id));
        for rejected in [
            "/api/attachments/avatar_1234?view=1",
            "/api/attachments/ra_0123456789abcdef0123456789abcdeg?view=1",
            "/api/attachments/ra_0123456789ABCDEF0123456789ABCDEF?view=1",
            "/api/attachments/ra_0123456789abcdef0123456789abcdef?download=1",
            "/api/attachments/ra_0123456789abcdef0123456789abcdef?view=1&extra=1",
        ] {
            assert_eq!(room_appearance_asset_id(rejected), None, "{rejected}");
        }
    }

    #[test]
    fn strict_decode_rejects_noncanonical_text() {
        let mut value = serde_json::to_value(RoomSettings::defaults("General"))
            .unwrap_or_else(|error| panic!("settings value: {error}"));
        value["topic"] = json!(" line\nwrap ");
        assert!(serde_json::from_value::<RoomSettings>(value).is_err());
    }

    #[test]
    fn strict_decode_rejects_unknown_nested_settings_fields() {
        let mut value = serde_json::to_value(RoomSettings::defaults("General"))
            .unwrap_or_else(|error| panic!("settings value: {error}"));
        value["appearance"]["continuous"] = json!(true);
        assert!(serde_json::from_value::<RoomSettings>(value).is_err());
    }
}
