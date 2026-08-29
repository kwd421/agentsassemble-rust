use uuid::Uuid;

use crate::runtime::ProviderTurnRequest;

pub(super) fn terminal_prompt(
    request: &ProviderTurnRequest,
    transcript_nonce: Uuid,
    helper: &str,
) -> String {
    let random_instruction = request
        .room_observation
        .as_ref()
        .and_then(|observation| observation.room_tool_ingress.as_ref())
        .map_or_else(String::new, |_| {
            format!(
                " For official game randomness, run exactly one `{helper} roll '<NdS±M>'` or `{helper} choose '<json-options>'` command and wait for its result."
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
    format!(
        "{}\n\n<agentsassemble-transport turn=\"{}\" launch=\"{transcript_nonce}\">Antigravity room transport: first run `{helper} help`, then run `{helper} read`.{media_instruction}{random_instruction} Finish with exactly one `{helper} speak 'message'`, `{helper} speak-to agent-id 'message'`, or `{helper} decline reason`. Run one helper command per terminal tool call and set `BypassSandbox` to `true`; the managed PreToolUse hook allows that only for this exact private helper prefix. Ordinary assistant final text is not a room publication.</agentsassemble-transport>",
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
        ProviderAttachmentReadIngress, ProviderRoomObservation, runtime::ProviderTurnRequest,
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
                room_tool_ingress: None,
            }),
        };
        let prompt = terminal_prompt(&request, Uuid::nil(), "agentsassemble-room");
        assert!(prompt.contains("agentsassemble-room media <attachment-id>"));
        assert!(prompt.contains("returned private path"));
        assert!(prompt.contains("set `BypassSandbox` to `true`"));
    }
}
