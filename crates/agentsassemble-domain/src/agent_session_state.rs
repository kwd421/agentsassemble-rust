use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentSessionStatus {
    Available,
    Attached,
    Unavailable,
    Detached,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AgentRuntimeStatus {
    Stopped,
    Starting,
    Idle,
    Busy,
    Paused,
    Stopping,
    Disconnected,
    Recovering,
    Error,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
pub enum AgentTurnPhase {
    #[serde(rename = "")]
    #[default]
    None,
    #[serde(rename = "thinking")]
    Thinking,
    #[serde(rename = "streaming")]
    Streaming,
}

impl AgentTurnPhase {
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentLifecycleAction {
    #[serde(rename = "")]
    #[default]
    None,
    #[serde(rename = "start")]
    Start,
    #[serde(rename = "stop")]
    Stop,
}

impl AgentLifecycleAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Start => "start",
            Self::Stop => "stop",
        }
    }

    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentLifecycleIntentStatus {
    #[serde(rename = "")]
    #[default]
    None,
    #[serde(rename = "prepared")]
    Prepared,
    #[serde(rename = "effect_inflight")]
    EffectInflight,
    #[serde(rename = "unconfirmed")]
    Unconfirmed,
    #[serde(rename = "effect_applied")]
    EffectApplied,
}

impl AgentLifecycleIntentStatus {
    #[must_use]
    pub const fn is_none(self) -> bool {
        matches!(self, Self::None)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AgentSessionStatus,
        AgentTurnPhase,
    };

    #[test]
    fn agent_session_vocabulary_preserves_serialized_strings() {
        for (action, encoded) in [
            (AgentLifecycleAction::None, "\"\""),
            (AgentLifecycleAction::Start, "\"start\""),
            (AgentLifecycleAction::Stop, "\"stop\""),
        ] {
            assert_eq!(
                serde_json::to_string(&action)
                    .unwrap_or_else(|error| panic!("serialize lifecycle action: {error}")),
                encoded
            );
            assert_eq!(
                serde_json::from_str::<AgentLifecycleAction>(encoded)
                    .unwrap_or_else(|error| panic!("deserialize lifecycle action: {error}")),
                action
            );
        }
        for (status, encoded) in [
            (AgentLifecycleIntentStatus::None, "\"\""),
            (AgentLifecycleIntentStatus::Prepared, "\"prepared\""),
            (
                AgentLifecycleIntentStatus::EffectInflight,
                "\"effect_inflight\"",
            ),
            (AgentLifecycleIntentStatus::Unconfirmed, "\"unconfirmed\""),
            (
                AgentLifecycleIntentStatus::EffectApplied,
                "\"effect_applied\"",
            ),
        ] {
            assert_eq!(
                serde_json::to_string(&status)
                    .unwrap_or_else(|error| panic!("serialize lifecycle status: {error}")),
                encoded
            );
            assert_eq!(
                serde_json::from_str::<AgentLifecycleIntentStatus>(encoded)
                    .unwrap_or_else(|error| panic!("deserialize lifecycle status: {error}")),
                status
            );
        }
        assert_eq!(
            serde_json::to_value(AgentSessionStatus::Attached)
                .unwrap_or_else(|error| panic!("serialize Agent Session status: {error}")),
            "attached"
        );
        assert_eq!(
            serde_json::to_value(AgentRuntimeStatus::Recovering)
                .unwrap_or_else(|error| panic!("serialize Agent runtime status: {error}")),
            "recovering"
        );
        assert_eq!(
            serde_json::to_value(AgentTurnPhase::None)
                .unwrap_or_else(|error| panic!("serialize Agent turn phase: {error}")),
            ""
        );
    }
}
