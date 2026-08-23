use agentsassemble_domain::DurableAgentSession;

use crate::turn_queue::event_id_queue_is_canonical;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct InvalidTurnAuthority;

pub(crate) fn active_turn_authority(
    session: &DurableAgentSession,
) -> Result<bool, InvalidTurnAuthority> {
    if !event_id_queue_is_canonical(
        session
            .inflight_event_ids
            .iter()
            .chain(&session.pending_event_ids),
    ) {
        return Err(InvalidTurnAuthority);
    }
    let active = !session.public.active_turn_id.is_empty()
        && matches!(session.public.runtime_status.as_str(), "busy" | "stopping")
        && matches!(session.public.turn_phase.as_str(), "thinking" | "streaming")
        && !session.inflight_event_ids.is_empty()
        && session.inflight_event_ids.last() == Some(&session.active_source_event_id)
        && session.active_source_event_id == session.input_up_to_event_id
        && session.input_up_to_seq > 0;
    let clear = session.public.active_turn_id.is_empty()
        && session.public.runtime_status != "busy"
        && session.public.turn_phase.is_empty()
        && session.inflight_event_ids.is_empty()
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
    use agentsassemble_domain::DurableAgentSession;

    use super::active_turn_authority;

    #[test]
    fn busy_state_without_exact_turn_tuple_is_rejected() {
        let mut session = serde_json::from_value::<DurableAgentSession>(serde_json::json!({
            "room_id": "general",
            "session_id": "agent-1",
            "participant_id": "agent-1",
            "display_name": "Agent",
            "status": "attached",
            "runtime_status": "busy",
            "enabled": true,
            "provider_kind": "test",
            "runtime_kind": "test",
            "connection_kind": "test",
            "external_owned": false,
            "process_ownership": "server",
            "model": "test",
            "reasoning_effort": "",
            "service_tier": "",
            "variant": "",
            "execution_harness": "builtin",
            "permission_mode": "meeting_read_only",
            "max_output_tokens": 0,
            "catalog_revision": "test",
            "transport": "test",
            "last_seen_event_id": "",
            "last_seen_seq": 0,
            "last_provider_sync_event_id": "",
            "last_provider_sync_seq": 0,
            "bootstrap_cutoff_seq": 0,
            "turn_count": 0,
            "created_at": "2026-08-23T00:00:00Z",
            "updated_at": "2026-08-23T00:00:00Z",
            "workspace": "/test",
            "runtime_profile_key": "test"
        }))
        .unwrap_or_else(|error| panic!("decode turn authority fixture: {error}"));
        assert!(active_turn_authority(&session).is_err());
        "idle".clone_into(&mut session.public.runtime_status);
        assert_eq!(active_turn_authority(&session), Ok(false));
    }
}
