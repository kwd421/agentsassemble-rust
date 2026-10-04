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

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct OwnerDeviceDescription {
    pub device_name: String,
    pub browser: String,
    pub os: String,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum OwnerDeviceKind {
    Host,
    Owner,
    Pairing,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct OwnerDeviceSession {
    pub session_id: String,
    pub device_name: String,
    pub browser: String,
    pub os: String,
    pub last_connected_at: Option<i64>,
    pub current: bool,
    pub connected: Option<bool>,
    pub revocable: bool,
    pub kind: OwnerDeviceKind,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct OwnerDevices {
    pub sessions: Vec<OwnerDeviceSession>,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(tag = "scope", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevokeOwnerDevices {
    Session { session_id: uuid::Uuid },
    All {},
}
