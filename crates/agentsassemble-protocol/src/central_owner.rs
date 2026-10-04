use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CentralOwnerSessionGrant {
    pub session_token: String,
    pub server_id: String,
    pub generation: i64,
    pub expires_at: i64,
    pub session_expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum CentralOwnerSessionStatus {
    Active { expires_at: i64 },
    Retrying { expires_at: i64 },
    Ended { reason: CentralOwnerSessionEnd },
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum CentralOwnerSessionEnd {
    Revoked,
    Expired,
    Unavailable,
}
