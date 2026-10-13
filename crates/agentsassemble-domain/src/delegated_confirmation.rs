//! Owner decisions bind to the host's immutable action, never a replacement command.
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema, TS)]
#[serde(deny_unknown_fields)]
pub struct DeleteTextChannel {
    pub request_id: String,
    pub channel_id: String,
    pub expected_settings_revision: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ConfirmedRoomAction {
    DeleteTextChannel(DeleteTextChannel),
    StartAgent(ConfirmedAgentStart),
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
impl ConfirmedRoomAction {
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::DeleteTextChannel(r) => &r.request_id,
            Self::StartAgent(r) => &r.request_id,
        }
    }
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::DeleteTextChannel(_) => "request_delete_text_channel",
            Self::StartAgent(_) => "start_agent",
        }
    }
}
