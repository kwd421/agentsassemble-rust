use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use uuid::Uuid;

use crate::{QueuedRoomInput, persona::PersonaAssetSummary};

pub const LOCAL_OPERATOR_USER_ID: &str = "operator-local-user";
pub const LOCAL_OPERATOR_PARTICIPANT_ID: &str = "operator-local";
pub const CURRENT_RUNTIME_PROFILE_VERSION: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RoomStatus {
    Active,
    Closed,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ParticipantRole {
    Human,
    Director,
    Implementer,
    Reviewer,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct Participant {
    pub room_id: String,
    pub participant_id: String,
    pub display_name: String,
    pub avatar_image_url: String,
    pub participant_type: String,
    pub status: ParticipantStatus,
    pub role: ParticipantRole,
    pub owner_id: String,
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
        Self::for_principal(client_kind, invite_scope, true)
    }

    #[must_use]
    pub fn for_principal(
        client_kind: ClientKind,
        invite_scope: InviteScope,
        is_operator: bool,
    ) -> Self {
        let bridge = client_kind == ClientKind::AgentBridge;
        let writable = invite_scope == InviteScope::ReadWrite;
        Self {
            room_history: !bridge,
            room_vote_summary: !bridge,
            room_random: writable && !bridge,
            message_send: writable && !bridge,
            message_modify: writable && !bridge,
            room_manage: is_operator,
            room_delete: is_operator,
            participant_leave: !bridge,
            participant_kick: is_operator,
            participant_mute: is_operator,
            agent_control: is_operator,
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
    pub is_operator: bool,
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
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
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
#[serde(deny_unknown_fields)]
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
    pub persona_card_id: Box<str>,
    pub persona_card: Option<Box<PersonaAssetSummary>>,
    pub transport: String,
    pub last_seen_event_id: String,
    pub last_seen_seq: i64,
    pub last_provider_sync_event_id: String,
    pub last_provider_sync_seq: i64,
    pub bootstrap_cutoff_seq: i64,
    pub turn_count: u64,
    pub active_turn_id: String,
    pub turn_phase: String,
    pub last_error: String,
    pub last_error_code: String,
    pub recovery_required: bool,
    pub provider_session_active: bool,
    pub provider_session_reused: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DurableAgentSession {
    #[serde(flatten)]
    pub public: AgentSession,
    pub executable: String,
    pub executable_identity: String,
    pub workspace: String,
    pub workspace_identity: String,
    pub runtime_profile_key: String,
    pub runtime_profile_version: u32,
    pub provider_session_id: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub turn_generation: u64,
    pub schedule_requested: bool,
    pub pending_inputs: Vec<QueuedRoomInput>,
    pub inflight_inputs: Vec<QueuedRoomInput>,
    pub active_source_event_id: String,
    pub input_up_to_event_id: String,
    pub input_up_to_seq: i64,
    pub lifecycle_intent_action: String,
    pub lifecycle_intent_id: String,
    pub lifecycle_intent_status: String,
}

#[derive(Deserialize)]
struct RawDurableAgentSession {
    #[serde(flatten)]
    public: AgentSession,
    executable: String,
    executable_identity: String,
    workspace: String,
    workspace_identity: String,
    runtime_profile_key: String,
    runtime_profile_version: u32,
    provider_session_id: String,
    runtime_handle_id: String,
    runtime_owner_id: String,
    runtime_lease_token: String,
    turn_generation: u64,
    schedule_requested: bool,
    pending_inputs: Vec<QueuedRoomInput>,
    inflight_inputs: Vec<QueuedRoomInput>,
    active_source_event_id: String,
    input_up_to_event_id: String,
    input_up_to_seq: i64,
    lifecycle_intent_action: String,
    lifecycle_intent_id: String,
    lifecycle_intent_status: String,
    #[serde(flatten)]
    unknown: BTreeMap<String, Value>,
}

impl<'de> Deserialize<'de> for DurableAgentSession {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawDurableAgentSession::deserialize(deserializer)?;
        if let Some(field) = raw.unknown.keys().next() {
            return Err(serde::de::Error::custom(format!(
                "unknown Agent Session field `{field}`"
            )));
        }
        Ok(Self {
            public: raw.public,
            executable: raw.executable,
            executable_identity: raw.executable_identity,
            workspace: raw.workspace,
            workspace_identity: raw.workspace_identity,
            runtime_profile_key: raw.runtime_profile_key,
            runtime_profile_version: raw.runtime_profile_version,
            provider_session_id: raw.provider_session_id,
            runtime_handle_id: raw.runtime_handle_id,
            runtime_owner_id: raw.runtime_owner_id,
            runtime_lease_token: raw.runtime_lease_token,
            turn_generation: raw.turn_generation,
            schedule_requested: raw.schedule_requested,
            pending_inputs: raw.pending_inputs,
            inflight_inputs: raw.inflight_inputs,
            active_source_event_id: raw.active_source_event_id,
            input_up_to_event_id: raw.input_up_to_event_id,
            input_up_to_seq: raw.input_up_to_seq,
            lifecycle_intent_action: raw.lifecycle_intent_action,
            lifecycle_intent_id: raw.lifecycle_intent_id,
            lifecycle_intent_status: raw.lifecycle_intent_status,
        })
    }
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
    pub persona_card_id: String,
    pub runtime_profile_key: String,
    pub transport: String,
}
