//! Initial session data has no process or turn authority; launch custody is reserved separately.
use crate::{
    AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeStatus, AgentSession,
    AgentSessionDraft, AgentSessionStatus, AgentTurnPhase, CURRENT_RUNTIME_PROFILE_VERSION,
    DurableAgentSession,
};
use chrono::{DateTime, Utc};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AgentRuntimeCustody {
    Server,
    External,
}

impl DurableAgentSession {
    /// Shared initial durable state; runtime custody is supplied only by its actual owner.
    #[must_use]
    pub fn without_runtime(public: AgentSession) -> Self {
        DurableAgentSession {
            public,
            executable: String::new(),
            executable_identity: String::new(),
            workspace: String::new(),
            workspace_identity: String::new(),
            provider_endpoint: String::new(),
            runtime_profile_key: String::new(),
            runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
            provider_session_id: String::new(),
            runtime_handle_id: String::new(),
            runtime_owner_id: String::new(),
            runtime_lease_token: String::new(),
            turn_generation: 0,
            schedule_requested: false,
            pending_inputs: Vec::new(),
            inflight_inputs: Vec::new(),
            active_source_event_id: String::new(),
            input_up_to_event_id: String::new(),
            input_up_to_seq: 0,
            lifecycle_intent_action: AgentLifecycleAction::None,
            lifecycle_intent_id: String::new(),
            lifecycle_intent_status: AgentLifecycleIntentStatus::None,
        }
    }
}

impl AgentSessionDraft {
    /// Builds local initial state only; the owning admission transaction grants room membership.
    #[must_use]
    pub fn initial_session(
        &self,
        room_id: &str,
        custody: AgentRuntimeCustody,
        now: DateTime<Utc>,
    ) -> DurableAgentSession {
        let public = AgentSession {
            avatar_image_url: String::new(),
            room_id: room_id.to_owned(),
            session_id: self.agent_id.clone(),
            participant_id: self.agent_id.clone(),
            display_name: self.display_name.clone(),
            status: AgentSessionStatus::Available,
            runtime_status: AgentRuntimeStatus::Stopped,
            enabled: false,
            provider_kind: self.provider_kind.clone(),
            runtime_kind: self.runtime_kind.clone(),
            connection_kind: self.connection_kind.clone(),
            external_owned: custody == AgentRuntimeCustody::External,
            process_ownership: match custody {
                AgentRuntimeCustody::Server => "server",
                AgentRuntimeCustody::External => "external",
            }
            .to_owned(),
            model: self.model.clone(),
            reasoning_effort: self.reasoning_effort.clone(),
            service_tier: self.service_tier.clone(),
            variant: self.variant.clone(),
            execution_harness: self.execution_harness.clone(),
            permission_mode: self.permission_mode.clone(),
            max_output_tokens: self.max_output_tokens,
            catalog_revision: self.catalog_revision.clone(),
            persona_card_id: self.persona_card_id.clone().into_boxed_str(),
            persona_card: None,
            transport: self.transport.clone(),
            last_seen_event_id: String::new(),
            last_seen_seq: 0,
            last_provider_sync_event_id: String::new(),
            last_provider_sync_seq: 0,
            bootstrap_cutoff_seq: 0,
            turn_count: 0,
            active_turn_id: String::new(),
            turn_phase: AgentTurnPhase::None,
            last_error: String::new(),
            last_error_code: String::new(),
            recovery_required: false,
            provider_session_active: false,
            provider_session_reused: false,
            created_at: now,
            updated_at: now,
        };
        let mut session = DurableAgentSession::without_runtime(public);
        session.executable.clone_from(&self.executable);
        session
            .executable_identity
            .clone_from(&self.executable_identity);
        session.workspace.clone_from(&self.workspace);
        session
            .workspace_identity
            .clone_from(&self.workspace_identity);
        session
            .provider_endpoint
            .clone_from(&self.provider_endpoint);
        session
            .runtime_profile_key
            .clone_from(&self.runtime_profile_key);
        session
    }
}
