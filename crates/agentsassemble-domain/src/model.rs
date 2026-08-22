use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use ts_rs::TS;
use uuid::Uuid;

pub const LOCAL_OPERATOR_USER_ID: &str = "operator-local-user";
pub const LOCAL_OPERATOR_PARTICIPANT_ID: &str = "operator-local";
pub const CURRENT_RUNTIME_PROFILE_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RoomStatus {
    Active,
    Closed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Room {
    pub room_id: String,
    pub room_uid: Uuid,
    pub label: String,
    pub status: RoomStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Room {
    #[must_use]
    pub fn new(room_id: String, label: String, now: DateTime<Utc>) -> Self {
        Self {
            room_id,
            room_uid: Uuid::new_v4(),
            label,
            status: RoomStatus::Active,
            created_at: now,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantStatus {
    Joined,
    Left,
    Kicked,
    Exported,
    Detached,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Participant {
    pub room_id: String,
    pub participant_id: String,
    pub display_name: String,
    pub participant_type: String,
    pub status: ParticipantStatus,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub owner_id: String,
    #[serde(default)]
    pub muted: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ClientKind {
    Browser,
    AgentBridge,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InviteScope {
    ReadWrite,
    ReadOnly,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[allow(clippy::struct_excessive_bools)] // Wire capabilities are independent named permissions.
pub struct CapabilitySet {
    #[serde(rename = "room.history")]
    pub room_history: bool,
    #[serde(rename = "room.vote.summary")]
    pub room_vote_summary: bool,
    #[serde(rename = "room.random")]
    pub room_random: bool,
    #[serde(rename = "message.send")]
    pub message_send: bool,
    #[serde(rename = "message.modify")]
    pub message_modify: bool,
    #[serde(rename = "room.manage")]
    pub room_manage: bool,
    #[serde(rename = "room.delete")]
    pub room_delete: bool,
    #[serde(rename = "participant.leave")]
    pub participant_leave: bool,
    #[serde(rename = "participant.kick")]
    pub participant_kick: bool,
    #[serde(rename = "participant.mute")]
    pub participant_mute: bool,
    #[serde(rename = "agent.control")]
    pub agent_control: bool,
    #[serde(rename = "provider.request.resolve")]
    pub provider_request_resolve: bool,
    #[serde(rename = "bridge.report")]
    pub bridge_report: bool,
    #[serde(rename = "bridge.publish")]
    pub bridge_publish: bool,
}

impl CapabilitySet {
    #[must_use]
    pub fn local_operator(client_kind: ClientKind, invite_scope: InviteScope) -> Self {
        let bridge = client_kind == ClientKind::AgentBridge;
        let writable = invite_scope == InviteScope::ReadWrite;
        Self {
            room_history: !bridge,
            room_vote_summary: !bridge,
            room_random: writable && !bridge,
            message_send: writable && !bridge,
            message_modify: writable && !bridge,
            room_manage: true,
            room_delete: true,
            participant_leave: !bridge,
            participant_kick: true,
            participant_mute: true,
            agent_control: true,
            provider_request_resolve: writable && !bridge,
            bridge_report: bridge,
            bridge_publish: bridge && writable,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AuthenticatedPrincipal {
    pub principal_id: String,
    pub participant_id: String,
    pub display_name: String,
    pub room_id: String,
    pub client_kind: ClientKind,
    pub invite_scope: InviteScope,
    pub capabilities: CapabilitySet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Actor {
    pub participant_id: String,
    pub participant_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct RoomEvent {
    pub v: u32,
    pub id: String,
    pub seq: i64,
    pub created_at: DateTime<Utc>,
    pub room_id: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub actor: Actor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub participant_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay_depth: Option<u32>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoomAppearance {
    pub banner_preset: String,
    pub banner_image_url: String,
    pub icon_image_url: String,
    pub icon_label: String,
    pub invite_scope: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoomSettings {
    pub label: String,
    pub topic: String,
    pub appearance: RoomAppearance,
    pub conversation_mode: String,
    pub tool_mode: String,
    pub ordered_exclude_previous_speaker: bool,
    pub max_relay_turns: u32,
    pub channels: Vec<Value>,
    pub activity_plugin: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PublicRoomSettings {
    pub settings_revision: String,
    pub label: String,
    pub topic: String,
    pub appearance: RoomAppearance,
    pub conversation_mode: String,
    pub tool_mode: String,
    pub ordered_exclude_previous_speaker: bool,
    pub max_relay_turns: u32,
    pub channels: Vec<Value>,
    pub activity_plugin: String,
}

impl RoomSettings {
    #[must_use]
    pub fn defaults(label: String) -> Self {
        Self {
            label,
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
            max_relay_turns: 6,
            channels: Vec::new(),
            activity_plugin: String::new(),
        }
    }
}

/// Serializes room settings and attaches their public content revision.
///
/// # Errors
///
/// Returns the serialization error if a settings field cannot be encoded.
pub fn public_settings(settings: &RoomSettings) -> Result<PublicRoomSettings, serde_json::Error> {
    let canonical = serde_json::to_vec(settings)?;
    let revision = format!("room-settings-v1-{:x}", Sha256::digest(canonical));
    Ok(PublicRoomSettings {
        settings_revision: revision,
        label: settings.label.clone(),
        topic: settings.topic.clone(),
        appearance: settings.appearance.clone(),
        conversation_mode: settings.conversation_mode.clone(),
        tool_mode: settings.tool_mode.clone(),
        ordered_exclude_previous_speaker: settings.ordered_exclude_previous_speaker,
        max_relay_turns: settings.max_relay_turns,
        channels: settings.channels.clone(),
        activity_plugin: settings.activity_plugin.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotMode {
    Initial,
    Resume,
    Gap,
    Bridge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderCatalog {
    pub status: String,
    pub catalog_revision: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discovered_at: String,
    pub providers: Vec<ProviderAvailability>,
}

impl Default for ProviderCatalog {
    fn default() -> Self {
        Self {
            status: "ready".to_owned(),
            catalog_revision: String::new(),
            discovered_at: String::new(),
            providers: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderControlOption {
    pub value: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub metadata: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ProviderControl {
    pub key: String,
    pub label: String,
    pub kind: String,
    pub options: Vec<ProviderControlOption>,
    pub default_value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[allow(clippy::struct_excessive_bools)] // Public provider capabilities are independent facts.
pub struct ProviderAvailability {
    pub id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub runtime_kind: String,
    pub catalog_group: String,
    pub workspace_required: bool,
    pub connection_kind: String,
    #[serde(skip)]
    #[ts(skip)]
    pub executable: String,
    #[serde(skip)]
    #[ts(skip)]
    pub executable_identity: String,
    pub default_model: String,
    pub interactive: bool,
    pub startable: bool,
    pub available: bool,
    pub discovery_status: String,
    pub catalog_source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discovery_error_code: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub discovery_error: String,
    pub login_available: bool,
    pub login_label: String,
    pub login_flow: String,
    pub controls: Vec<ProviderControl>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[allow(clippy::struct_excessive_bools)] // Public runtime observations are independent wire facts.
pub struct AgentSession {
    pub room_id: String,
    pub session_id: String,
    pub participant_id: String,
    pub display_name: String,
    pub status: String,
    pub runtime_status: String,
    pub enabled: bool,
    pub provider_kind: String,
    pub runtime_kind: String,
    pub connection_kind: String,
    pub external_owned: bool,
    pub process_ownership: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub variant: String,
    pub execution_harness: String,
    pub permission_mode: String,
    pub max_output_tokens: u32,
    pub catalog_revision: String,
    pub transport: String,
    pub last_seen_event_id: String,
    pub last_seen_seq: i64,
    pub last_provider_sync_event_id: String,
    pub last_provider_sync_seq: i64,
    pub bootstrap_cutoff_seq: i64,
    pub turn_count: u64,
    #[serde(default)]
    pub active_turn_id: String,
    #[serde(default)]
    pub turn_phase: String,
    #[serde(default)]
    pub last_error: String,
    #[serde(default)]
    pub last_error_code: String,
    #[serde(default)]
    pub recovery_required: bool,
    #[serde(default)]
    pub provider_session_active: bool,
    #[serde(default)]
    pub provider_session_reused: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableAgentSession {
    #[serde(flatten)]
    pub public: AgentSession,
    #[serde(default)]
    pub executable: String,
    #[serde(default)]
    pub executable_identity: String,
    pub workspace: String,
    #[serde(default)]
    pub workspace_identity: String,
    pub runtime_profile_key: String,
    #[serde(default)]
    pub runtime_profile_version: u32,
    #[serde(default)]
    pub provider_session_id: String,
    #[serde(default)]
    pub runtime_handle_id: String,
    #[serde(default)]
    pub runtime_owner_id: String,
    #[serde(default)]
    pub pending_event_ids: Vec<String>,
    #[serde(default)]
    pub inflight_event_ids: Vec<String>,
    #[serde(default)]
    pub lifecycle_intent_action: String,
    #[serde(default)]
    pub lifecycle_intent_id: String,
    #[serde(default)]
    pub lifecycle_intent_status: String,
}

impl DurableAgentSession {
    #[must_use]
    pub fn public(&self) -> AgentSession {
        self.public.clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSessionDraft {
    pub agent_id: String,
    pub display_name: String,
    pub provider_kind: String,
    pub runtime_kind: String,
    pub executable: String,
    pub executable_identity: String,
    pub workspace: String,
    pub workspace_identity: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub variant: String,
    pub execution_harness: String,
    pub permission_mode: String,
    pub max_output_tokens: u32,
    pub catalog_revision: String,
    pub runtime_profile_key: String,
    pub transport: String,
}
