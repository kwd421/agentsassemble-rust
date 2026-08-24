use rmcp::schemars;
use serde::Deserialize;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct PublishMessage {
    pub(super) content: String,
    #[serde(default)]
    pub(super) next_agent_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct DeclineToSpeak {
    pub(super) reason_code: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct RollDice {
    pub(super) notation: String,
    #[serde(default)]
    pub(super) reason: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ChooseRandom {
    pub(super) options: Vec<String>,
    #[serde(default)]
    pub(super) reason: String,
}
