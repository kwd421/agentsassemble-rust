use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ReleaseHealthCheck {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ReleaseHealthReport {
    pub started_at: DateTime<Utc>,
    pub completed_at: DateTime<Utc>,
    pub results: Vec<ReleaseHealthResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ReleaseHealthResult {
    pub check_id: String,
    pub status: ReleaseHealthStatus,
    pub duration_seconds: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseHealthStatus {
    Passed,
    Failed,
    Unavailable,
    TimedOut,
    Cancelled,
    CleanupUnconfirmed,
    NotRun,
}
