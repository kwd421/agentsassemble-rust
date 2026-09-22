use crate::{AgentRuntimeStatus, AgentSessionStatus, AgentTurnPhase, VoteSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentActivity {
    pub participant_id: String,
    pub display_name: String,
    pub status: AgentSessionStatus,
    pub runtime_status: AgentRuntimeStatus,
    pub turn_phase: AgentTurnPhase,
    pub recovery_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationStatus {
    pub observed_at: String,
    pub agents: Vec<AgentActivity>,
    pub open_votes: Vec<VoteSummary>,
    /// Continue with this cursor even when a page contains only expired polls.
    pub next_before_seq: i64,
}
