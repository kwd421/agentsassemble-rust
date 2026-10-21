//! In-turn compaction for the built-in harness, after pi's: when a turn's
//! messages come near the model's context window, the older tool rounds are
//! summarized by the model and replaced by that summary. The room rules, the
//! turn input and the latest rounds stay exactly as they were.
use serde_json::{Value, json};

/// The window assumed when neither the catalog nor the provider's documents
/// give one: small, so an unknown model is compacted early rather than late.
pub(crate) const UNKNOWN_CONTEXT_TOKENS: u64 = 32_768;
/// Recent rounds kept word for word, at most (pi's `keepRecentTokens`).
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

/// Compaction starts at 75% of what the window leaves after the reply limit.
pub(crate) fn threshold(context_tokens: u64, max_output: u32) -> u64 {
    context_tokens.saturating_sub(u64::from(max_output)) / 4 * 3
}

/// What the next request costs: the provider-reported usage of the last reply
/// (prompt and completion, covering `messages[..covered]`) plus an estimate of
/// what was added after it. Without a report everything is estimated.
pub(crate) fn used_tokens(messages: &[Value], measured: Option<(usize, u64)>) -> u64 {
    match measured {
        Some((covered, tokens)) if covered <= messages.len() => {
            tokens.saturating_add(estimate_tokens(&messages[covered..]))
        }
        _ => estimate_tokens(messages),
    }
}

/// Where the kept messages start. A round is one assistant message and the
/// tool results that answer it, so a tool call never loses its result. The
/// latest round is always kept; `None` means nothing older can be folded.
pub(crate) fn cut_point(messages: &[Value], opening: usize, threshold: u64) -> Option<usize> {
    // A small window keeps less, so folding still frees room.
    let keep = KEEP_RECENT_TOKENS.min(threshold / 3);
    let starts: Vec<usize> = (opening..messages.len())
        .filter(|&index| messages[index]["role"] == "assistant")
        .collect();
    let mut cut = *starts.last()?;
    let mut kept = estimate_tokens(&messages[cut..]);
    for &start in starts.iter().rev().skip(1) {
        let size = estimate_tokens(&messages[start..cut]);
        if kept + size > keep {
            break;
        }
        kept += size;
        cut = start;
    }
    // Folding must take at least one round; refolding a lone earlier summary
    // would spend a model call without freeing room.
    (starts[0] < cut).then_some(cut)
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

/// At most this much of a summary is kept, from its start.
const MAX_SUMMARY_CHARACTERS: usize = 12_000;

pub(crate) fn bounded_summary(content: &str) -> Option<String> {
    let summary = content.trim();
    (!summary.is_empty()).then(|| summary.chars().take(MAX_SUMMARY_CHARACTERS).collect())
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
