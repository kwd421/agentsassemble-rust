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
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct DelegatedActionDecisionAck {
    pub confirmation_id: String,
    pub state: ConfirmationState,
    pub receipt: serde_json::Value,
}
