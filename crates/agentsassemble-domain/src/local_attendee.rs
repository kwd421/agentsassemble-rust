use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;
use uuid::Uuid;

/// A private local-operator request. Invitation and creation inputs are never public state.
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LocalAttendeeCreate {
    pub request_id: Uuid,
    pub invite_url: String,
    pub room_id: String,
    pub room_uid: Uuid,
    pub creation: Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum LocalAttendeePhase {
    Admitting,
    AdmissionUnresolved,
    Admitted,
    Starting,
    Running,
    Stopping,
    Stopped,
    Failed,
    CleanupUnconfirmed,
}

/// Local operation observation, with no invitation, provider credentials or local paths.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct LocalAttendeeStatus {
    pub request_id: Uuid,
    pub room_id: String,
    pub room_uid: Uuid,
    pub participant_id: Option<String>,
    pub phase: LocalAttendeePhase,
    pub error_code: Option<String>,
}
