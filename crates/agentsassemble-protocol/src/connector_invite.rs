use agentsassemble_domain::InviteScope;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateConnectorInviteRequest {
    pub request_id: String,
    pub scope: InviteScope,
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
