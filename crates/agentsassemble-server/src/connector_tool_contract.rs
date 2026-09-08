use rmcp::schemars;
use serde::Deserialize;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Join {
    pub(super) invite_url: String,
    #[serde(default)]
    pub(super) display_name: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Connection {
    #[serde(default)]
    pub(super) connection_id: String,
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
    pub(super) content: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteCreate {
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
pub(super) struct Roll {
    pub(super) notation: String,
    #[serde(default)]
    pub(super) reason: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct Choose {
    pub(super) options: Vec<String>,
    #[serde(default)]
    pub(super) reason: String,
    #[serde(default)]
    pub(super) connection_id: String,
}

fn all_channels() -> String {
    "all".to_owned()
}
