//! Strict direct-action payloads. Transport supplies every authority field.
macro_rules! request {
    ($name:ident { $($field:ident: $ty:ty),* }) => {
        #[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
        #[serde(deny_unknown_fields)]
        pub struct $name { pub request_id: String, $(pub $field: $ty),* }
    };
}
request!(CreateTextChannel { name: String });
request!(RenameTextChannel {
    channel_id: String,
    name: String,
    expected_settings_revision: String
});
request!(SetParticipantMute {
    participant_id: String,
    muted: bool
});

request!(StartAgent { agent_id: String });
request!(StopAgent { agent_id: String });

// Director-requested stopped-agent creation. The schema mirrors the owner's own
// agent.create fields except server-owned identity and intent: no agent_id,
// full_access_acknowledgement, start/start_now or session_id can be supplied.
// Creation is always stopped; starting is the separate existing-agent action.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    ts_rs::TS,
)]
#[serde(deny_unknown_fields)]
pub struct RequestCreateAgent {
    pub request_id: String,
    pub provider_id: String,
    pub catalog_revision: String,
    pub display_name: String,
    pub workspace: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub service_tier: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub persona_card_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub provider_endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub max_output_tokens: Option<u64>,
}

// Director-requested agent reconfiguration. The schema carries only the mutable
// change fields the owner's own `agent.configure` accepts: no acknowledgement and no
// configuration revision can be supplied (the sealed runtime profile key is private
// projection), so the request binds the agent's current configuration revision on the
// server and refuses `target_changed` when it moved before execution.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    ts_rs::TS,
)]
#[serde(deny_unknown_fields)]
pub struct RequestReconfigureAgent {
    pub request_id: String,
    pub agent_id: String,
    pub catalog_revision: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reasoning_effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub permission_mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub workspace: Option<String>,
}

// Director-requested agent removal. The schema carries only the request identity and
// the target agent id: the target's current creation identity is bound on the server
// and a missing, replaced or already-removed agent refuses `target_changed`.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    ts_rs::TS,
)]
#[serde(deny_unknown_fields)]
pub struct RequestRemoveAgent {
    pub request_id: String,
    pub agent_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectRoomAction {
    RequestDeleteTextChannel(crate::DeleteTextChannel),
    CreateTextChannel(CreateTextChannel),
    RenameTextChannel(RenameTextChannel),
    SetParticipantMute(SetParticipantMute),
    StartAgent(StartAgent),
    StopAgent(StopAgent),
    RequestCreateAgent(RequestCreateAgent),
    RequestReconfigureAgent(RequestReconfigureAgent),
    RequestRemoveAgent(RequestRemoveAgent),
}
impl DirectRoomAction {
    pub const NAMES: &[&str] = &[
        "request_delete_text_channel",
        "create_text_channel",
        "rename_text_channel",
        "set_participant_mute",
        "start_agent",
        "stop_agent",
        "request_create_agent",
        "request_reconfigure_agent",
        "request_remove_agent",
    ];
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::RequestDeleteTextChannel(r) => &r.request_id,
            Self::CreateTextChannel(r) => &r.request_id,
            Self::RenameTextChannel(r) => &r.request_id,
            Self::SetParticipantMute(r) => &r.request_id,
            Self::StartAgent(r) => &r.request_id,
            Self::StopAgent(r) => &r.request_id,
            Self::RequestCreateAgent(r) => &r.request_id,
            Self::RequestReconfigureAgent(r) => &r.request_id,
            Self::RequestRemoveAgent(r) => &r.request_id,
        }
    }
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::RequestDeleteTextChannel(_) => "request_delete_text_channel",
            Self::CreateTextChannel(_) => "create_text_channel",
            Self::RenameTextChannel(_) => "rename_text_channel",
            Self::SetParticipantMute(_) => "set_participant_mute",
            Self::StartAgent(_) => "start_agent",
            Self::StopAgent(_) => "stop_agent",
            Self::RequestCreateAgent(_) => "request_create_agent",
            Self::RequestReconfigureAgent(_) => "request_reconfigure_agent",
            Self::RequestRemoveAgent(_) => "request_remove_agent",
        }
    }
}
