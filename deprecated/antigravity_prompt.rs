use uuid::Uuid;

use crate::runtime::ProviderTurnRequest;

#[cfg(unix)]
const MESSAGE_ARGUMENT: &str = "'message'";
#[cfg(windows)]
const MESSAGE_ARGUMENT: &str = "\"message\"";
#[cfg(unix)]
const SEARCH_ARGUMENT: &str = "'<query>'";
#[cfg(windows)]
const SEARCH_ARGUMENT: &str = "\"<query>\"";
#[cfg(unix)]
const ROLL_ARGUMENT: &str = "'<NdS±M>'";
#[cfg(windows)]
const ROLL_ARGUMENT: &str = "\"<NdS±M>\"";
#[cfg(unix)]
const CHOOSE_ARGUMENT: &str = "'<json-options>'";
#[cfg(windows)]
const CHOOSE_ARGUMENT: &str = "<compact-json-options>";
#[cfg(unix)]
const VOTE_DEFINITION_ARGUMENT: &str = "'<json-definition>'";
#[cfg(windows)]
const VOTE_DEFINITION_ARGUMENT: &str = "<compact-json-definition>";
#[cfg(unix)]
const VOTE_CHOICE_ARGUMENT: &str = "'<choice>'";
#[cfg(windows)]
const VOTE_CHOICE_ARGUMENT: &str = "\"<choice>\"";

pub(super) fn terminal_prompt(
    request: &ProviderTurnRequest,
    transcript_nonce: Uuid,
    helper: &str,
) -> String {
    let random_instruction = request
        .room_observation
        .as_ref()
        .filter(|observation| observation.tabletop_tools)
        .map_or_else(String::new, |_| {
            format!(
                " For official game randomness, run exactly one `{helper} roll {ROLL_ARGUMENT}` or `{helper} choose {CHOOSE_ARGUMENT}` command and wait for its result."
            )
        });
    let media_instruction = request
        .room_observation
        .as_ref()
        .filter(|observation| !observation.attachment_ids.is_empty())
        .map_or_else(String::new, |_| {
            format!(
                " To inspect a listed attachment, run `{helper} media <attachment-id>` and open the returned private path with the appropriate native viewer or reader."
            )
        });
    let history_instruction = request
        .room_observation
        .as_ref()
        .filter(|observation| observation.room_tool_ingress.is_some())
        .map_or_else(String::new, |_| {
            format!(
                " To search older room messages, run `{helper} search {SEARCH_ARGUMENT}`; pass the returned `next_cursor` as a second argument for the next page, and run `{helper} context <event-id>` to read one result's bounded context."
            )
        });
    let vote_instruction = format!(
        " To create or update a room poll, finish with exactly one `{helper} vote-create {VOTE_DEFINITION_ARGUMENT}`, `{helper} vote-cast <vote-id> {VOTE_CHOICE_ARGUMENT}`, `{helper} vote-withdraw <vote-id>`, or `{helper} vote-close <vote-id>` command. A vote definition is one compact JSON object with `question`, `options`, and optional `duration_seconds`."
    );
    format!(
        "{}\n\n<agentsassemble-transport turn=\"{}\" launch=\"{transcript_nonce}\">Antigravity room transport: first run `{helper} help`, then run `{helper} read`.{media_instruction}{history_instruction}{random_instruction}{vote_instruction} Otherwise finish with exactly one `{helper} speak {MESSAGE_ARGUMENT}`, `{helper} speak-to agent-id {MESSAGE_ARGUMENT}`, or `{helper} decline reason`. Run one helper command per terminal tool call and set `BypassSandbox` to `true`; the managed PreToolUse hook allows that only for this exact private helper prefix. Ordinary assistant final text is not a room publication.</agentsassemble-transport>",
        request.input, request.turn_id,
    )
}

pub(super) fn model_matches(configured: &str, observed: &str) -> bool {
    let normalized = |value: &str| {
        value
            .to_ascii_lowercase()
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                }
            })
            .collect::<String>()
            .split('-')
            .filter(|part| !part.is_empty() && !matches!(*part, "low" | "medium" | "high"))
            .collect::<Vec<_>>()
            .join("-")
    };
    normalized(configured) == normalized(observed)
}

pub(super) fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::terminal_prompt;
    use crate::{
        ProviderAttachmentReadIngress, ProviderRoomObservation, ProviderRoomToolIngress,
        runtime::ProviderTurnRequest,
    };

    #[test]
    fn attachment_turn_teaches_the_exact_private_media_command() {
        let (attachment_ingress, _commands) = ProviderAttachmentReadIngress::channel(1);
        let request = ProviderTurnRequest {
            turn_id: "turn-1".to_owned(),
            turn_generation: 1,
            execution_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            input: "room wake".to_owned(),
            room_observation: Some(ProviderRoomObservation {
                session_id: "agent-1".to_owned(),
                input_up_to_seq: 1,
                view: "Attachment `ma_11111111111111111111111111111111`".to_owned(),
                attachment_ids: vec!["ma_11111111111111111111111111111111".to_owned()],
                attachment_ingress: Some(attachment_ingress),
                allowed_agent_ids: Vec::new(),
                tabletop_tools: false,
                room_tool_ingress: None,
            }),
        };
        let prompt = terminal_prompt(&request, Uuid::nil(), "agentsassemble-room");
        assert!(prompt.contains("agentsassemble-room media <attachment-id>"));
        assert!(prompt.contains("returned private path"));
        assert!(prompt.contains("set `BypassSandbox` to `true`"));
    }

    #[test]
    fn room_turn_teaches_history_without_enabling_tabletop_tools() {
        let (room_tool_ingress, _commands) = ProviderRoomToolIngress::channel(1);
        let request = ProviderTurnRequest {
            turn_id: "turn-1".to_owned(),
            turn_generation: 1,
            execution_id: "00000000-0000-4000-8000-000000000001".to_owned(),
            input: "room wake".to_owned(),
            room_observation: Some(ProviderRoomObservation {
                session_id: "agent-1".to_owned(),
                input_up_to_seq: 1,
                view: "#1 Human: find the old deployment".to_owned(),
                attachment_ids: Vec::new(),
                attachment_ingress: None,
                allowed_agent_ids: Vec::new(),
                tabletop_tools: false,
                room_tool_ingress: Some(room_tool_ingress),
            }),
        };
        let prompt = terminal_prompt(&request, Uuid::nil(), "agentsassemble-room");
        assert!(prompt.contains("agentsassemble-room search"));
        assert!(prompt.contains("agentsassemble-room context <event-id>"));
        assert!(prompt.contains("agentsassemble-room vote-create"));
        assert!(prompt.contains("agentsassemble-room vote-cast <vote-id>"));
        assert!(prompt.contains("agentsassemble-room vote-withdraw <vote-id>"));
        assert!(prompt.contains("agentsassemble-room vote-close <vote-id>"));
        assert!(prompt.contains("`question`, `options`, and optional `duration_seconds`"));
        assert!(!prompt.contains("official game randomness"));
    }
}
