//! In-turn compaction for the built-in harness, after pi's: when a turn's
//! messages come near the model's context window, the older tool rounds are
//! summarized by the model and replaced by that summary. The room rules, the
//! turn input and the latest rounds stay exactly as they were.
use serde_json::{Value, json};

/// Room left for the reply and the next tool round (pi's `reserveTokens`).
const RESERVE_TOKENS: u64 = 16_384;
/// Recent rounds kept word for word (pi's `keepRecentTokens`).
const KEEP_RECENT_TOKENS: u64 = 20_000;

const SUMMARY_INSTRUCTIONS: &str = "You summarize an agent's work in progress so it can continue in a smaller context. Write a concise summary of the work so far: what was done, what the tools returned that still matters, exact file paths, names, values and error codes, and what remains to do for the task. Do not invent anything and do not continue the work yourself.";

/// About three bytes of request JSON per token: it over-counts English and
/// roughly matches Korean, so the estimate errs toward compacting early.
pub(crate) fn estimate_tokens(messages: &[Value]) -> u64 {
    let bytes: usize = messages
        .iter()
        .map(|message| serde_json::to_vec(message).map_or(0, |bytes| bytes.len()))
        .sum();
    u64::try_from(bytes).unwrap_or(u64::MAX) / 3
}

pub(crate) fn needs_compaction(messages: &[Value], context_tokens: u64, max_output: u32) -> bool {
    let reserve = RESERVE_TOKENS.max(u64::from(max_output));
    estimate_tokens(messages) > context_tokens.saturating_sub(reserve)
}

/// Where the kept messages start. A round is one assistant message and the
/// tool results that answer it, so a tool call never loses its result. The
/// latest round is always kept; `None` means nothing older can be folded.
pub(crate) fn cut_point(messages: &[Value], opening: usize) -> Option<usize> {
    let starts: Vec<usize> = (opening..messages.len())
        .filter(|&index| messages[index]["role"] == "assistant")
        .collect();
    let mut cut = *starts.last()?;
    let mut kept = estimate_tokens(&messages[cut..]);
    for &start in starts.iter().rev().skip(1) {
        let size = estimate_tokens(&messages[start..cut]);
        if kept + size > KEEP_RECENT_TOKENS {
            break;
        }
        kept += size;
        cut = start;
    }
    (cut > opening).then_some(cut)
}

/// The request that asks the model for a summary of `messages[opening..cut]`.
pub(crate) fn summary_request(messages: &[Value], opening: usize, cut: usize) -> Vec<Value> {
    let task = messages[..opening]
        .iter()
        .rev()
        .find(|message| message["role"] == "user")
        .and_then(|message| message["content"].as_str())
        .unwrap_or_default();
    let mut work = String::new();
    for message in &messages[opening..cut] {
        let content = message["content"].as_str().unwrap_or_default();
        match message["role"].as_str() {
            Some("assistant") => {
                if !content.is_empty() {
                    work.push_str(&format!("Agent: {content}\n"));
                }
                for call in message["tool_calls"].as_array().into_iter().flatten() {
                    let function = &call["function"];
                    work.push_str(&format!(
                        "Called {}({})\n",
                        function["name"].as_str().unwrap_or_default(),
                        function["arguments"].as_str().unwrap_or_default()
                    ));
                }
            }
            Some("tool") => work.push_str(&format!("Result: {content}\n")),
            _ => work.push_str(&format!("Earlier summary: {content}\n")),
        }
    }
    vec![
        json!({"role": "system", "content": SUMMARY_INSTRUCTIONS}),
        json!({"role": "user", "content": format!("Task:\n{task}\n\nWork so far:\n{work}")}),
    ]
}

/// Replaces `messages[opening..cut]` with the model's summary of them.
pub(crate) fn fold(messages: &mut Vec<Value>, opening: usize, cut: usize, summary: &str) {
    messages.splice(
        opening..cut,
        [json!({
            "role": "user",
            "content": format!("Summary of the earlier work in this turn (older tool results were folded to fit the context):\n{summary}"),
        })],
    );
}

#[cfg(test)]
#[path = "turn_compaction_tests.rs"]
mod tests;
