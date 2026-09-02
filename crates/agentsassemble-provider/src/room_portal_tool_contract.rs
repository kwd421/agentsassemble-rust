use rmcp::schemars;
use serde::Deserialize;

pub(crate) const READ_DISCUSSION_TOOL: &str = "read_discussion";
pub(crate) const READ_ATTACHMENT_TOOL: &str = "read_attachment";
pub(crate) const SEARCH_MESSAGES_TOOL: &str = "search_messages";
pub(crate) const READ_MESSAGE_CONTEXT_TOOL: &str = "read_message_context";
pub(crate) const PUBLISH_MESSAGE_TOOL: &str = "publish_message";
pub(crate) const DECLINE_TO_SPEAK_TOOL: &str = "decline_to_speak";
pub(crate) const CREATE_VOTE_TOOL: &str = "create_vote";
pub(crate) const CAST_VOTE_TOOL: &str = "cast_vote";
pub(crate) const WITHDRAW_VOTE_TOOL: &str = "withdraw_vote";
pub(crate) const CLOSE_VOTE_TOOL: &str = "close_vote";
pub(crate) const ROLL_DICE_TOOL: &str = "roll_dice";
pub(crate) const CHOOSE_RANDOM_TOOL: &str = "choose_random";

pub(crate) const PROVIDER_ROOM_TOOL_NAMES: [&str; 12] = [
    READ_DISCUSSION_TOOL,
    READ_ATTACHMENT_TOOL,
    SEARCH_MESSAGES_TOOL,
    READ_MESSAGE_CONTEXT_TOOL,
    PUBLISH_MESSAGE_TOOL,
    DECLINE_TO_SPEAK_TOOL,
    CREATE_VOTE_TOOL,
    CAST_VOTE_TOOL,
    WITHDRAW_VOTE_TOOL,
    CLOSE_VOTE_TOOL,
    ROLL_DICE_TOOL,
    CHOOSE_RANDOM_TOOL,
];

pub(crate) const VOTE_TOOL_NAMES: [&str; 4] = [
    CREATE_VOTE_TOOL,
    CAST_VOTE_TOOL,
    WITHDRAW_VOTE_TOOL,
    CLOSE_VOTE_TOOL,
];

pub(crate) fn is_vote_tool(name: &str) -> bool {
    VOTE_TOOL_NAMES.contains(&name)
}

pub(crate) fn is_available_provider_tool(name: &str, tabletop_tools: bool) -> bool {
    PROVIDER_ROOM_TOOL_NAMES.contains(&name)
        && (tabletop_tools || !matches!(name, ROLL_DICE_TOOL | CHOOSE_RANDOM_TOOL))
}

pub(crate) fn is_terminal_provider_tool(name: &str) -> bool {
    matches!(name, PUBLISH_MESSAGE_TOOL | DECLINE_TO_SPEAK_TOOL) || is_vote_tool(name)
}

pub(crate) fn is_replay_unsafe_provider_tool(name: &str) -> bool {
    is_terminal_provider_tool(name) || matches!(name, ROLL_DICE_TOOL | CHOOSE_RANDOM_TOOL)
}

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

#[cfg(test)]
mod tests {
    use super::{
        PROVIDER_ROOM_TOOL_NAMES, READ_ATTACHMENT_TOOL, ROLL_DICE_TOOL, is_available_provider_tool,
        is_replay_unsafe_provider_tool, is_terminal_provider_tool,
    };

    #[test]
    fn provider_projection_uses_one_room_tool_policy() {
        assert!(PROVIDER_ROOM_TOOL_NAMES.contains(&READ_ATTACHMENT_TOOL));
        assert!(is_available_provider_tool(READ_ATTACHMENT_TOOL, false));
        assert!(!is_available_provider_tool(ROLL_DICE_TOOL, false));
        assert!(is_available_provider_tool(ROLL_DICE_TOOL, true));
        assert!(!is_terminal_provider_tool(ROLL_DICE_TOOL));
        assert!(is_replay_unsafe_provider_tool(ROLL_DICE_TOOL));
        assert!(!is_available_provider_tool("unknown", true));
    }
}
