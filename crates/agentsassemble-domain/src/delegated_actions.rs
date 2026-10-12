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

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum DirectRoomAction {
    CreateTextChannel(CreateTextChannel),
    RenameTextChannel(RenameTextChannel),
    SetParticipantMute(SetParticipantMute),
    StartAgent(StartAgent),
    StopAgent(StopAgent),
}
impl DirectRoomAction {
    pub const NAMES: &[&str] = &[
        "create_text_channel",
        "rename_text_channel",
        "set_participant_mute",
        "start_agent",
        "stop_agent",
    ];
    #[must_use]
    pub fn request_id(&self) -> &str {
        match self {
            Self::CreateTextChannel(r) => &r.request_id,
            Self::RenameTextChannel(r) => &r.request_id,
            Self::SetParticipantMute(r) => &r.request_id,
            Self::StartAgent(r) => &r.request_id,
            Self::StopAgent(r) => &r.request_id,
        }
    }
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::CreateTextChannel(_) => "create_text_channel",
            Self::RenameTextChannel(_) => "rename_text_channel",
            Self::SetParticipantMute(_) => "set_participant_mute",
            Self::StartAgent(_) => "start_agent",
            Self::StopAgent(_) => "stop_agent",
        }
    }
}
