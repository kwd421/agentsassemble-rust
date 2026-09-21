use agentsassemble_domain::InviteScope;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Where a connector invite link is opened from.
///
/// A public link uses the ready public ingress origin. A local link uses the runtime's own
/// loopback listener: only processes on this machine can reach it, and it stops resolving once
/// the runtime restarts on another port. The invite credential itself is the same either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum InviteReach {
    Public,
    Local,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateConnectorInviteRequest {
    pub request_id: String,
    pub scope: InviteScope,
    pub reach: InviteReach,
}

// Secret-bearing HTTP result: deliberately no Debug implementation.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreatedConnectorInvite {
    pub request_id: String,
    pub room_uid: String,
    pub invite_id: String,
    pub expires_at: String,
    pub join_url: String,
}
