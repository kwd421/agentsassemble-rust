use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateFriendAttendeeInvite {
    pub request_id: String,
    pub friend_id: String,
}

#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateCompanionAttendeeInvite {
    pub request_id: String,
    pub provider: String,
    pub display_name: String,
}

// This result contains a private one-use credential; never derive Debug or put it in room events.
#[derive(Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AttendeeEntryPacket {
    pub request_id: String,
    pub room_id: String,
    pub room_uid: String,
    pub invite_id: String,
    pub expires_at: String,
    pub display_name: String,
    pub provider: String,
    pub attend_command: String,
    pub join_url: String,
}
