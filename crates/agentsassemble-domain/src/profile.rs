use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

const DEFAULT_DISPLAY_NAME: &str = "SeiNel";
const DEFAULT_HANDLE: &str = "seinel.";
const DEFAULT_CUSTOM_STATUS: &str = "AgentsAssemble";
const DEFAULT_AVATAR_LABEL: &str = "나";
const DEFAULT_ACCENT_COLOR: &str = "#5865f2";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserProfile {
    pub revision: i64,
    pub display_name: String,
    pub handle: String,
    pub status: String,
    pub custom_status: String,
    pub avatar_label: String,
    pub avatar_image_url: String,
    pub banner_preset: String,
    pub accent_color: String,
    pub mic_muted: bool,
    pub deafened: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct UserProfilePatch {
    pub display_name: Option<String>,
    pub handle: Option<String>,
    pub status: Option<String>,
    pub custom_status: Option<String>,
    pub avatar_label: Option<String>,
    pub avatar_image_url: Option<String>,
    pub banner_preset: Option<String>,
    pub accent_color: Option<String>,
    pub mic_muted: Option<bool>,
    pub deafened: Option<bool>,
}

impl UserProfile {
    #[must_use]
    pub fn defaults(now: DateTime<Utc>) -> Self {
        Self {
            revision: 1,
            display_name: DEFAULT_DISPLAY_NAME.to_owned(),
            handle: DEFAULT_HANDLE.to_owned(),
            status: "online".to_owned(),
            custom_status: DEFAULT_CUSTOM_STATUS.to_owned(),
            avatar_label: DEFAULT_AVATAR_LABEL.to_owned(),
            avatar_image_url: String::new(),
            banner_preset: "default".to_owned(),
            accent_color: DEFAULT_ACCENT_COLOR.to_owned(),
            mic_muted: true,
            deafened: false,
            created_at: now,
            updated_at: now,
        }
    }

    pub fn apply_patch(&mut self, patch: UserProfilePatch, now: DateTime<Utc>) -> bool {
        let previous = self.clone();
        if let Some(value) = patch.display_name {
            self.display_name = defaulted_text(&value, 120, DEFAULT_DISPLAY_NAME);
        }
        if let Some(value) = patch.handle {
            self.handle = defaulted_text(&value, 120, DEFAULT_HANDLE);
        }
        if let Some(value) = patch.status {
            let value = clean_text(&value, 24);
            self.status = if ["online", "idle", "dnd", "offline"].contains(&value.as_str()) {
                value
            } else {
                "online".to_owned()
            };
        }
        if let Some(value) = patch.custom_status {
            self.custom_status = defaulted_text(&value, 160, DEFAULT_CUSTOM_STATUS);
        }
        if let Some(value) = patch.avatar_label {
            let value = clean_text(&value, 2).to_uppercase();
            self.avatar_label = if value.is_empty() {
                DEFAULT_AVATAR_LABEL.to_owned()
            } else {
                value
            };
        }
        if let Some(value) = patch.avatar_image_url {
            self.avatar_image_url = canonical_avatar_url(&value).unwrap_or_default();
        }
        if let Some(value) = patch.banner_preset {
            let value = clean_text(&value, 24);
            self.banner_preset =
                if ["default", "forest", "midnight", "ember", "custom"].contains(&value.as_str()) {
                    value
                } else {
                    "default".to_owned()
                };
        }
        if let Some(value) = patch.accent_color {
            let value = clean_text(&value, 16);
            self.accent_color = if valid_accent_color(&value) {
                value.to_ascii_lowercase()
            } else {
                DEFAULT_ACCENT_COLOR.to_owned()
            };
        }
        if let Some(value) = patch.mic_muted {
            self.mic_muted = value;
        }
        if let Some(value) = patch.deafened {
            self.deafened = value;
        }
        if profile_values_equal(self, &previous) {
            return false;
        }
        self.revision = previous.revision.saturating_add(1).max(1);
        self.updated_at = now;
        true
    }
}

fn profile_values_equal(left: &UserProfile, right: &UserProfile) -> bool {
    left.display_name == right.display_name
        && left.handle == right.handle
        && left.status == right.status
        && left.custom_status == right.custom_status
        && left.avatar_label == right.avatar_label
        && left.avatar_image_url == right.avatar_image_url
        && left.banner_preset == right.banner_preset
        && left.accent_color == right.accent_color
        && left.mic_muted == right.mic_muted
        && left.deafened == right.deafened
}

#[must_use]
pub fn canonical_avatar_url(value: &str) -> Option<String> {
    let value = clean_text(value, 240);
    let attachment_id = value
        .strip_prefix("/api/attachments/")?
        .strip_suffix("?view=1")?;
    valid_attachment_id(attachment_id).then(|| format!("/api/attachments/{attachment_id}?view=1"))
}

#[must_use]
pub fn avatar_attachment_id(value: &str) -> Option<&str> {
    value
        .strip_prefix("/api/attachments/")?
        .strip_suffix("?view=1")
        .filter(|value| valid_attachment_id(value))
}

fn clean_text(value: &str, limit: usize) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(limit)
        .collect()
}

fn defaulted_text(value: &str, limit: usize, default: &str) -> String {
    let value = clean_text(value, limit);
    if value.is_empty() {
        default.to_owned()
    } else {
        value
    }
}

fn valid_attachment_id(value: &str) -> bool {
    (8..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

fn valid_accent_color(value: &str) -> bool {
    value.len() == 7
        && value.starts_with('#')
        && value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::{UserProfile, UserProfilePatch};

    #[test]
    fn patch_normalizes_original_profile_shapes_without_crossing_authority() {
        let mut profile = UserProfile::defaults(Utc::now());
        let changed = profile.apply_patch(
            UserProfilePatch {
                display_name: Some("  New\n Name  ".to_owned()),
                status: Some("not-a-status".to_owned()),
                avatar_image_url: Some("/api/attachments/avatar_1234?view=1".to_owned()),
                accent_color: Some("#A0B1C2".to_owned()),
                custom_status: Some(String::new()),
                mic_muted: Some(false),
                ..UserProfilePatch::default()
            },
            Utc::now(),
        );
        assert!(changed);
        assert_eq!(profile.display_name, "New Name");
        assert_eq!(profile.status, "online");
        assert_eq!(
            profile.avatar_image_url,
            "/api/attachments/avatar_1234?view=1"
        );
        assert_eq!(profile.accent_color, "#a0b1c2");
        assert_eq!(profile.custom_status, "AgentsAssemble");
        assert!(!profile.mic_muted);
        assert_eq!(profile.revision, 2);
        let updated_at = profile.updated_at;
        assert!(!profile.apply_patch(
            UserProfilePatch {
                display_name: Some("New Name".to_owned()),
                ..UserProfilePatch::default()
            },
            Utc::now(),
        ));
        assert_eq!(profile.revision, 2);
        assert_eq!(profile.updated_at, updated_at);
    }
}
