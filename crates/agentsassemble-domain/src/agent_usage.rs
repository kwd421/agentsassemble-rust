//! How much an API AI used its model: what one turn reports, and the running
//! totals its owner sees, like the footer of DeepSeek's own harness.
use serde::{Deserialize, Serialize};

/// One turn's model use, summed over its tool rounds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentTurnUsage {
    /// Model calls in the turn (one per tool round).
    pub steps: u64,
    pub prompt_tokens: u64,
    /// Prompt tokens the provider served from its cache.
    pub cached_prompt_tokens: u64,
    pub completion_tokens: u64,
    /// The last call's prompt size, for how full the context is.
    pub last_prompt_tokens: u64,
    /// The context window the turn ran with.
    pub context_tokens: u64,
    /// Time spent waiting on the model's answers, in milliseconds.
    pub model_ms: u64,
}

/// An AI's totals since it was created.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentUsageTotals {
    pub turns: u64,
    pub steps: u64,
    pub prompt_tokens: u64,
    pub cached_prompt_tokens: u64,
    pub completion_tokens: u64,
    pub last_prompt_tokens: u64,
    pub context_tokens: u64,
    pub model_ms: u64,
}
