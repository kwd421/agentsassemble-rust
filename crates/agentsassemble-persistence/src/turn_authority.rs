use agentsassemble_domain::DurableAgentSession;

use crate::turn_queue::room_input_queue_is_canonical;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidTurnAuthority;

pub(crate) fn active_turn_authority(
    session: &DurableAgentSession,
) -> Result<bool, InvalidTurnAuthority> {
    if !room_input_queue_is_canonical(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    ) {
        return Err(InvalidTurnAuthority);
    }
    let active = !session.public.active_turn_id.is_empty()
        && matches!(session.public.runtime_status.as_str(), "busy" | "stopping")
        && matches!(session.public.turn_phase.as_str(), "thinking" | "streaming")
        && !session.inflight_inputs.is_empty()
        && session
            .inflight_inputs
            .last()
            .is_some_and(|input| input.event_id == session.active_source_event_id)
        && session.active_source_event_id == session.input_up_to_event_id
        && session.input_up_to_seq > 0;
    let clear = session.public.active_turn_id.is_empty()
        && session.public.runtime_status != "busy"
        && session.public.turn_phase.is_empty()
        && session.inflight_inputs.is_empty()
        && session.active_source_event_id.is_empty()
        && session.input_up_to_event_id.is_empty()
        && session.input_up_to_seq == 0;
    match (active, clear) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err(InvalidTurnAuthority),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{AgentSession, DurableAgentSession};
    use chrono::Utc;

    use super::active_turn_authority;

    #[test]
    fn busy_state_without_exact_turn_tuple_is_rejected() {
        let now = Utc::now();
        let mut session = DurableAgentSession {
            public: AgentSession {
                room_id: "general".to_owned(),
                session_id: "agent-1".to_owned(),
                participant_id: "agent-1".to_owned(),
                display_name: "Agent".to_owned(),
                status: "attached".to_owned(),
                runtime_status: "busy".to_owned(),
                enabled: true,
                provider_kind: "test".to_owned(),
                runtime_kind: "test".to_owned(),
                connection_kind: "test".to_owned(),
                external_owned: false,
                process_ownership: "server".to_owned(),
                model: "test".to_owned(),
                reasoning_effort: String::new(),
                service_tier: String::new(),
                variant: String::new(),
                execution_harness: "builtin".to_owned(),
                permission_mode: "meeting_read_only".to_owned(),
                max_output_tokens: 0,
                catalog_revision: "test".to_owned(),
                transport: "test".to_owned(),
                last_seen_event_id: String::new(),
                last_seen_seq: 0,
                last_provider_sync_event_id: String::new(),
                last_provider_sync_seq: 0,
                bootstrap_cutoff_seq: 0,
                turn_count: 0,
                active_turn_id: String::new(),
                turn_phase: String::new(),
                last_error: String::new(),
                last_error_code: String::new(),
                recovery_required: false,
                provider_session_active: true,
                provider_session_reused: false,
                created_at: now,
                updated_at: now,
            },
            executable: "/test/provider".to_owned(),
            executable_identity: "test-provider".to_owned(),
            workspace: "/test".to_owned(),
            workspace_identity: "test-workspace".to_owned(),
            runtime_profile_key: "test".to_owned(),
            runtime_profile_version: 3,
            provider_session_id: "test-session".to_owned(),
            runtime_handle_id: "test-handle".to_owned(),
            runtime_owner_id: "test-owner".to_owned(),
            runtime_lease_token: "test-lease-generation".to_owned(),
            turn_generation: 0,
            schedule_requested: false,
            pending_inputs: Vec::new(),
            inflight_inputs: Vec::new(),
            active_source_event_id: String::new(),
            input_up_to_event_id: String::new(),
            input_up_to_seq: 0,
            lifecycle_intent_action: String::new(),
            lifecycle_intent_id: String::new(),
            lifecycle_intent_status: String::new(),
        };
        assert!(active_turn_authority(&session).is_err());
        "idle".clone_into(&mut session.public.runtime_status);
        assert_eq!(active_turn_authority(&session), Ok(false));
    }
}
