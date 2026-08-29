use agentsassemble_domain::UserProfile;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalControlRequest {
    InspectBootstrap {
        request_id: String,
    },
    InitializeBootstrap {
        request_id: String,
        display_name: String,
    },
    IssueTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueOperatorHttpTicket {
        request_id: String,
    },
    IssuePreferencesReadTicket {
        request_id: String,
        meeting_id: String,
    },
    IssuePreferencesWriteTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueMessagePinsReadTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueMessagePinsWriteTicket {
        request_id: String,
        meeting_id: String,
    },
    IssueHumanInviteCreateTicket {
        request_id: String,
        server_id: String,
        authority_lineage_id: String,
        meeting_id: String,
        room_uid: String,
    },
    IssueHumanInviteRevokeTicket {
        request_id: String,
        server_id: String,
        authority_lineage_id: String,
        meeting_id: String,
        room_uid: String,
    },
    IssueAppearanceUploadTicket {
        request_id: String,
        server_id: String,
        authority_lineage_id: String,
        meeting_id: String,
        room_uid: String,
    },
    IssueAppearancePendingReadTicket {
        request_id: String,
        server_id: String,
        authority_lineage_id: String,
        meeting_id: String,
        room_uid: String,
        asset_id: String,
    },
    IssueAppearanceBoundReadTicket {
        request_id: String,
        server_id: String,
        authority_lineage_id: String,
        meeting_id: String,
        room_uid: String,
        asset_id: String,
    },
    IssueSettingsDirectoryReadTicket {
        request_id: String,
    },
    IssueCentralRegistrationTicket {
        request_id: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalBootstrapPhase {
    Empty,
    Initializing,
    Complete,
    RepairRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalBootstrapGrant {
    pub phase: LocalBootstrapPhase,
    pub authority_lineage_id: String,
    pub server_id: String,
    pub server_product_surface_revision: u32,
    pub server_product_surface_digest: String,
    pub profile: Option<UserProfile>,
    pub deduplicated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum LocalControlResponse {
    BootstrapOk {
        request_id: String,
        bootstrap: Box<LocalBootstrapGrant>,
    },
    Ok {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
        server_proof_key: String,
    },
    OperatorHttpOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    PreferencesReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    PreferencesWriteOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    MessagePinsReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    MessagePinsWriteOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    HumanInviteCreateOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    HumanInviteRevokeOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    AppearanceUploadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    AppearancePendingReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    AppearanceBoundReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    SettingsDirectoryReadOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
    },
    CentralRegistrationOk {
        request_id: String,
        ticket: String,
        ttl_seconds: u64,
        server_id: String,
        host_public_key_x: String,
        host_key_fingerprint: String,
    },
    Error {
        request_id: String,
        code: String,
        message: String,
    },
}
