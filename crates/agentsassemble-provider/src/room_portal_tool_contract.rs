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
pub(super) struct CreateVote {
    pub(super) question: String,
    pub(super) options: Vec<String>,
    #[serde(default)]
    pub(super) duration_seconds: u32,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CastVote {
    pub(super) vote_id: String,
    pub(super) choice: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct VoteTarget {
    pub(super) vote_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadAttachment {
    pub(super) attachment_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct SearchMessages {
    pub(super) query: String,
    #[serde(default)]
    pub(super) cursor: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct ReadMessageContext {
    pub(super) event_id: String,
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
