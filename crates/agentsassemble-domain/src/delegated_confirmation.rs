//! Owner decisions bind to the host's immutable action, never a replacement command.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct DeleteTextChannel {
    pub request_id: String,
    pub channel_id: String,
    /// Settings revision at request time. Informational only: approval binds
    /// the target (channel id plus card name), never this revision. Kept on
    /// the wire so old web clients keep validating; new clients accept the
    /// action with or without it (non-empty when present). Stored rows from
    /// the revision-less host parse as `None` and are honored the same way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub expected_settings_revision: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfirmedRoomAction {
    DeleteTextChannel(DeleteTextChannel),
    StartAgent(ConfirmedAgentStart),
    CreateAgent(ConfirmedAgentCreate),
    ReconfigureAgent(ConfirmedAgentReconfigure),
    RemoveAgent(ConfirmedAgentRemove),
    KickParticipant(ConfirmedParticipantKick),
    CreateHumanInvite(ConfirmedHumanInvite),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationState {
    Pending,
    Declined,
    Expired,
    Cancelled,
    Executing,
    Succeeded,
    Failed,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DelegatedActionRequest {
    pub confirmation_id: String,
    pub owner_participant_id: String,
    pub owner_display_name: String,
    pub delegate_participant_id: String,
    pub delegate_display_name: String,
    pub action: ConfirmedRoomAction,
    pub target_name: String,
    pub consequences: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub state: ConfirmationState,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationDecision {
    Approve,
    Decline,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DecideDelegatedAction {
    pub confirmation_id: String,
    pub decision: ConfirmationDecision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub full_access_acknowledged: Option<bool>,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DelegatedActionDecisionAck {
    pub confirmation_id: String,
    pub state: ConfirmationState,
    pub receipt: serde_json::Value,
    /// Present only in the live acknowledgement of an approved HUMAN room-invite
    /// decision, exactly once. Never stored, replayed or delivered to the AI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub invite: Option<DelegatedInviteAck>,
}

/// The one-time human room-invite credential delivered only to the bound owner in the
/// live acknowledgement of the owner's own decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DelegatedInviteAck {
    pub invite_id: String,
    pub invite_token: String,
    pub join_code: String,
    pub join_url: String,
    pub display_name: String,
    pub invite_scope: String,
    pub max_uses: i64,
    pub expires_at: String,
}

/// Owner-private fields of requested/updated room event envelopes.
#[derive(Debug, Clone, Serialize, Deserialize, TS)]
pub struct DelegatedActionRequestEvent {
    pub visibility: String,
    pub owner_id: String,
    pub delegated_action_request: DelegatedActionRequest,
}

/// Correlated, secret-free outcome available to the originating AI in room status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegatedActionOutcome {
    pub request_id: String,
    pub confirmation_id: String,
    pub state: ConfirmationState,
    pub receipt: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AgentStartConfiguration {
    pub provider_kind: String,
    pub runtime_kind: String,
    pub connection_kind: String,
    pub model: String,
    pub reasoning_effort: String,
    pub service_tier: String,
    pub variant: String,
    pub execution_harness: String,
    pub permission_mode: String,
    pub max_output_tokens: u32,
    pub catalog_revision: String,
    pub persona_card_id: String,
    pub transport: String,
    pub working_folder: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedAgentStart {
    pub request_id: String,
    pub agent_id: String,
    pub configuration: AgentStartConfiguration,
    pub expected_configuration_revision: String,
    pub expected_created_at: DateTime<Utc>,
}
/// Owner-confirmed stopped-agent creation. `params` are the AI-supplied fields as
/// received; `draft` is the catalog-selected stopped session bound at request time,
/// including every server-resolved default and the deterministic agent identity.
/// No provider credential appears here; the draft carries only session authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedAgentCreate {
    pub request_id: String,
    pub params: crate::RequestCreateAgent,
    pub draft: crate::AgentSessionDraft,
}
/// Owner-confirmed agent reconfiguration. `params` are the AI-supplied change fields
/// as received; `expected_configuration_revision` seals the agent's configuration as
/// it stood at request time (the private runtime profile key, never readable by an
/// AI); `draft` is the catalog-resolved resulting profile bound at request time.
/// No acknowledgement or configuration revision can be supplied by the AI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedAgentReconfigure {
    pub request_id: String,
    pub agent_id: String,
    pub params: crate::RequestReconfigureAgent,
    pub expected_configuration_revision: String,
    pub draft: crate::AgentSessionDraft,
}
/// Owner-confirmed agent removal. `params` are the AI-supplied request identity and
/// target id as received; `expected_created_at` seals the target agent's creation
/// identity, so a missing, replaced or already-removed agent refuses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedAgentRemove {
    pub request_id: String,
    pub agent_id: String,
    pub params: crate::RequestRemoveAgent,
    pub expected_created_at: DateTime<Utc>,
}
/// Owner-confirmed human participant kick. `params` are the AI-supplied request
/// identity and target id as received; `expected_created_at` seals the target
/// person's admission identity and `expected_updated_at` seals its latest state, so
/// a missing, replaced, re-admitted or already-removed person refuses. The admission
/// core moves `updated_at` on every return to `Joined` while leave keeps
/// `created_at`, so a leave-and-rejoin inside the confirmation window refuses even
/// though the creation identity is unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedParticipantKick {
    pub request_id: String,
    pub participant_id: String,
    pub params: crate::RequestKickParticipant,
    pub expected_created_at: DateTime<Utc>,
    #[serde(default)]
    pub expected_updated_at: DateTime<Utc>,
}
/// Owner-confirmed HUMAN room invite. `params` are the AI-supplied invite fields as
/// received; request time and approval both run the owner's own invite-policy
/// validation, so approval cannot mint a different scope, use limit or expiry. No
/// credential is bound here, and the minted link is never stored in this request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConfirmedHumanInvite {
    pub request_id: String,
    pub params: crate::RequestCreateInvite,
}
impl ConfirmedRoomAction {
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::DeleteTextChannel(r) => &r.request_id,
            Self::StartAgent(r) => &r.request_id,
            Self::CreateAgent(r) => &r.request_id,
            Self::ReconfigureAgent(r) => &r.request_id,
            Self::RemoveAgent(r) => &r.request_id,
            Self::KickParticipant(r) => &r.request_id,
            Self::CreateHumanInvite(r) => &r.request_id,
        }
    }
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DeleteTextChannel(_) => "request_delete_text_channel",
            Self::StartAgent(_) => "start_agent",
            Self::CreateAgent(_) => "request_create_agent",
            Self::ReconfigureAgent(_) => "request_reconfigure_agent",
            Self::RemoveAgent(_) => "request_remove_agent",
            Self::KickParticipant(_) => "request_kick_participant",
            Self::CreateHumanInvite(_) => "request_create_invite",
        }
    }
}
