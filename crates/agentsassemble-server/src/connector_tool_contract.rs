use rmcp::schemars;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Join {
    pub(super) invite_url: String,
    #[serde(default)]
    pub(super) display_name: String,
    /// Remote MCP: retain the prepared private handle before confirming admission.
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Connection {
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
    #[serde(default)]
    pub(super) connection_id: String,
    /// Explicitly replace pending wait observations with this bounded snapshot.
    #[serde(default)]
    pub(super) resync: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Leave {
    #[serde(default)]
    pub(super) connection_id: String,
    /// Release only after receiving the successful leave receipt.
    #[serde(default)]
    pub(super) release_receipt: bool,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    pub(super) query: String,
    #[serde(default = "all_channels")]
    pub(super) channel_id: String,
    #[serde(default)]
    pub(super) cursor: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Context {
    pub(super) channel_id: String,
    #[serde(rename = "event_id")]
    pub(super) event: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Say {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    pub(super) content: String,
    /// Optional event UUID of an earlier lobby message to reply to.
    #[serde(default)]
    pub(super) reply_to_event_id: Option<String>,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteCreate {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    pub(super) question: String,
    pub(super) options: Vec<String>,
    #[serde(default)]
    pub(super) duration_seconds: u32,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteCast {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    pub(super) vote_id: String,
    pub(super) choice: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteTarget {
    pub(super) vote_id: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteMutation {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    #[serde(rename = "vote_id")]
    pub(super) vote: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Roll {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    pub(super) notation: String,
    #[serde(default)]
    pub(super) reason: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Choose {
    /// Generate a UUID before the first call; retain it unchanged for retries of this intent.
    pub(super) request_id: String,
    pub(super) options: Vec<String>,
    #[serde(default)]
    pub(super) reason: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

fn all_channels() -> String {
    "all".to_owned()
}
