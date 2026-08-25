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
    format!(
        "{}\n\n<agentsassemble-transport turn=\"{}\" launch=\"{transcript_nonce}\">Antigravity room transport: first run `{helper} help`, then run `{helper} read`.{random_instruction} Finish with exactly one `{helper} speak 'message'`, `{helper} speak-to agent-id 'message'`, or `{helper} decline reason`. Run one helper command per terminal tool call. Ordinary assistant final text is not a room publication.</agentsassemble-transport>",
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
