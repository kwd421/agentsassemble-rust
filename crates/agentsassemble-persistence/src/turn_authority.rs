use agentsassemble_domain::DurableAgentSession;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{PersistenceError, turn_queue::room_input_queue_is_canonical};

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

pub(crate) async fn require_provider_room_tool_authority(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    input_up_to_seq: i64,
    turn_generation: u64,
    execution_id: &str,
) -> Result<(), PersistenceError> {
    if active_turn_authority(session) != Ok(true)
        || session.public.active_turn_id != turn_id
        || session.input_up_to_seq != input_up_to_seq
        || session.turn_generation != turn_generation
        || turn_generation == 0
        || Uuid::parse_str(execution_id).is_err()
        || session.public.status != "attached"
        || session.public.runtime_status != "busy"
        || !session.public.enabled
        || !session.public.provider_session_active
        || session.public.process_ownership != "server"
        || session.runtime_handle_id.is_empty()
        || session.runtime_owner_id.is_empty()
        || session.runtime_lease_token.is_empty()
    {
        return Err(stale_provider_turn());
    }
    let generation = i64::try_from(turn_generation).map_err(|_| stale_provider_turn())?;
    let exact_execution = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM provider_turn_executions execution \
         WHERE execution.room_id = ? AND execution.session_id = ? \
         AND execution.turn_generation = ? AND execution.execution_id = ? \
         AND execution.turn_id = ? AND execution.phase IN ('start_dispatching', 'running') \
         AND execution.start_dispatch_nonce != '' AND execution.runtime_handle_id = ? \
         AND execution.runtime_owner_id = ? AND execution.runtime_lease_token = ? \
         AND NOT EXISTS (SELECT 1 FROM provider_turn_effects effect \
           WHERE effect.room_id = execution.room_id \
           AND effect.session_id = execution.session_id \
           AND effect.turn_generation = execution.turn_generation \
           AND effect.phase != 'finalized'))",
    )
    .bind(&session.public.room_id)
    .bind(&session.public.session_id)
    .bind(generation)
    .bind(execution_id)
    .bind(turn_id)
    .bind(&session.runtime_handle_id)
    .bind(&session.runtime_owner_id)
    .bind(&session.runtime_lease_token)
    .fetch_one(&mut **transaction)
    .await?
        != 0;
    if !exact_execution {
        return Err(stale_provider_turn());
    }
    Ok(())
}

fn stale_provider_turn() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_provider_turn",
        message: "Room tool result no longer matches the active provider turn.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AgentSession, CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession,
    };
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
                persona_card_id: Box::default(),
                persona_card: None,
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
            runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
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
