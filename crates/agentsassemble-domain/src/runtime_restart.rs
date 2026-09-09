use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeRestartPhase {
    Quiescing,
    Draining,
    Recovering,
    Completed,
    Failed,
    Aborted,
}

impl RuntimeRestartPhase {
    #[must_use]
    pub const fn blocks_admission(self) -> bool {
        matches!(self, Self::Quiescing | Self::Draining | Self::Recovering)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRestartReceipt {
    pub operation_id: String,
    pub phase: RuntimeRestartPhase,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRestartStatus {
    pub supported: bool,
    pub operation: Option<RuntimeRestartReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RuntimeRestartRequest {
    pub operation_id: Uuid,
}
