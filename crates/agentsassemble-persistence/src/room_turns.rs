use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, DurableAgentSession, MessageSend, RoomEvent,
    RoomInputDeliveryKind, canonical_payload_hash, clean_message, has_visible_text,
    prepare_message_event, redact_persisted_diagnostic_text,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle::{load_session, save_session},
    command_admission::admit_non_lifecycle_command,
    room_write_budget::command_size,
    turn_queue::merge_room_inputs,
};

#[derive(Debug, Clone)]
pub struct AgentTurnAssignment {
    pub session: DurableAgentSession,
    pub turn_id: String,
    pub delivery_kind: RoomInputDeliveryKind,
    pub provider_input: String,
    pub room_view: String,
    pub room_agent_ids: Vec<String>,
    pub tabletop_tools: bool,
}

#[derive(Debug, Clone)]
pub struct RoomCommandMutation {
    pub outcome: CommandOutcome,
    pub assignments: Vec<AgentTurnAssignment>,
}

#[derive(Debug, Clone)]
pub struct AgentTurnCommit {
    pub events: Vec<RoomEvent>,
    pub next_assignments: Vec<AgentTurnAssignment>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderTurnAuthority<'a> {
    pub turn_id: &'a str,
    pub provider_turn_id: &'a str,
    pub provider_session_id: Option<&'a str>,
}

pub(super) struct PreparedAssignment {
    assignment: AgentTurnAssignment,
    events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Assigns the oldest recovered ordered-floor input after a session becomes startable.
    ///
    /// # Errors
    ///
    /// Returns room-state or storage failures without publishing an external effect.
    pub async fn assign_pending_turn(
        &self,
        room_id: &str,
    ) -> Result<Option<AgentTurnCommit>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, room_id).await?;
        let prepared = assign_available_pending(&mut transaction, &room, &settings).await?;
        if prepared.is_empty() {
            transaction.commit().await?;
            return Ok(None);
        }
        let mut events = Vec::new();
        let mut assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            assignments.push(item.assignment);
        }
        transaction.commit().await?;
        Ok(Some(AgentTurnCommit {
            events,
            next_assignments: assignments,
        }))
    }

    /// Records a post-commit room-floor progression failure without changing the
    /// already committed command result.
    ///
    /// # Errors
    ///
    /// Returns a storage failure when the public error event cannot be committed.
    pub async fn record_floor_progression_failure(
        &self,
        room_id: &str,
        assignment_error_code: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let assignment_error_code = public_assignment_error_code(assignment_error_code);
        let mut transaction = self.pool.begin().await?;
        let _ = load_active_room(&mut transaction, room_id).await?;
        let event = RoomEvent {
            v: 1,
            id: Uuid::new_v4().to_string(),
            seq: next_sequence(&mut transaction, room_id).await?,
            created_at: Utc::now(),
            room_id: room_id.to_owned(),
            event_type: "error".to_owned(),
            actor: Actor {
                participant_id: "room-system".to_owned(),
                participant_type: "system".to_owned(),
            },
            participant_id: None,
            participant_type: Some("system".to_owned()),
            actor_id: Some("room-system".to_owned()),
            actor_type: Some("system".to_owned()),
            display_name: Some("Room System".to_owned()),
            content: Some("Queued Agent Session work could not be advanced.".to_owned()),
            message_kind: None,
            extra: BTreeMap::from([
                ("error_code".to_owned(), json!("floor_progression_failed")),
                (
                    "diagnostics".to_owned(),
                    json!({"assignment_error_code": assignment_error_code}),
                ),
            ]),
        };
        insert_event(&mut transaction, &event).await?;
        transaction.commit().await?;
        Ok(vec![event])
    }

    /// Commits a room message and its ordered-floor queue/assignment atomically.
    ///
    /// # Errors
    ///
    /// Returns authorization, idempotency, room-state, or storage failures.
    pub async fn execute_message_with_turn(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, &principal.room_id).await?;
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
            command_size(request_id, action, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        if action != "message.send" {
            return Err(rejected(
                "unsupported_action",
                format!("Unsupported room command: {action}"),
            ));
        }
        let command = MessageSend::from_payload(payload).map_err(rejection)?;
        let participant = load_participant(
            &mut transaction,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        let sequence = next_sequence(&mut transaction, &principal.room_id).await?;
        let event = prepare_message_event(principal, &participant, &command, sequence, Utc::now())
            .map_err(rejection)?;
        insert_event(&mut transaction, &event).await?;
        route_message(&mut transaction, &settings, &event).await?;
        let prepared = assign_available_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![event.clone()];
        let mut assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            assignments.push(item.assignment);
        }
        let result = json!({"event": event, "event_seq": sequence});
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(action)
        .bind(payload_hash)
        .bind(serde_json::to_string(&result)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(RoomCommandMutation {
            outcome: CommandOutcome {
                result,
                event,
                events,
                deduplicated: false,
            },
            assignments,
        })
    }

    /// Keeps the existing persistence API for callers that do not execute provider effects.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::execute_message_with_turn`].
    pub async fn execute_message(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        self.execute_message_with_turn(principal, request_id, action, payload)
            .await
            .map(|mutation| mutation.outcome)
    }

    /// Atomically publishes one provider final and advances the ordered floor.
    ///
    /// # Errors
    ///
    /// Returns an exact active-turn conflict or storage failure.
    pub async fn complete_agent_turn(
        &self,
        room_id: &str,
        session_id: &str,
        authority: ProviderTurnAuthority<'_>,
        content: &str,
        target_agent_id: &str,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let ProviderTurnAuthority {
            turn_id,
            provider_turn_id,
            provider_session_id,
        } = authority;
        validate_identifier(provider_turn_id, "provider_turn_invalid")?;
        let content = clean_message(content, 12_000);
        if !has_visible_text(&content) {
            return Err(rejected(
                "provider_turn_output_missing",
                "The provider turn completed without a room-visible final message.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, room_id).await?;
        let mut session = load_session(&mut transaction, room_id, session_id).await?;
        require_active_turn(&session, turn_id)?;
        validate_input_cursor(&mut transaction, &session).await?;
        validate_publication_target(&mut transaction, &session, target_agent_id).await?;
        apply_provider_session_transition(&mut session, provider_session_id)?;
        let source_event_id = session.active_source_event_id.clone();
        let input_event_id = session.input_up_to_event_id.clone();
        let input_seq = session.input_up_to_seq;
        let final_event = agent_final_event(
            &mut transaction,
            &session,
            turn_id,
            provider_turn_id,
            &source_event_id,
            content,
            target_agent_id,
        )
        .await?;
        let finished = turn_finished_event(
            &mut transaction,
            &session,
            turn_id,
            "completed",
            Some(provider_turn_id),
            None,
        )
        .await?;
        complete_session_state(&mut session, &input_event_id, input_seq);
        save_session(&mut transaction, &session).await?;
        let state = session_state_event(&mut transaction, &session).await?;
        route_message(&mut transaction, &settings, &final_event).await?;
        let prepared = assign_available_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![final_event, finished, state];
        let mut next_assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            next_assignments.push(item.assignment);
        }
        transaction.commit().await?;
        Ok(AgentTurnCommit {
            events,
            next_assignments,
        })
    }

    /// Atomically records an explicit provider decline and advances the ordered floor.
    ///
    /// # Errors
    ///
    /// Returns an exact active-turn conflict, invalid decline, or storage failure.
    pub async fn decline_agent_turn(
        &self,
        room_id: &str,
        session_id: &str,
        authority: ProviderTurnAuthority<'_>,
        reason_code: &str,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let ProviderTurnAuthority {
            turn_id,
            provider_turn_id,
            provider_session_id,
        } = authority;
        validate_identifier(provider_turn_id, "provider_turn_invalid")?;
        if !matches!(
            reason_code,
            "nothing_useful_to_add" | "not_addressed" | "duplicate"
        ) {
            return Err(rejected(
                "invalid_decline_reason",
                "The provider decline reason is unsupported.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, room_id).await?;
        let mut session = load_session(&mut transaction, room_id, session_id).await?;
        require_active_turn(&session, turn_id)?;
        validate_input_cursor(&mut transaction, &session).await?;
        apply_provider_session_transition(&mut session, provider_session_id)?;
        let input_event_id = session.input_up_to_event_id.clone();
        let input_seq = session.input_up_to_seq;
        let finished = turn_finished_event(
            &mut transaction,
            &session,
            turn_id,
            "declined",
            Some(provider_turn_id),
            Some(reason_code),
        )
        .await?;
        complete_session_state(&mut session, &input_event_id, input_seq);
        save_session(&mut transaction, &session).await?;
        let state = session_state_event(&mut transaction, &session).await?;
        let prepared = assign_available_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![finished, state];
        let mut next_assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            next_assignments.push(item.assignment);
        }
        transaction.commit().await?;
        Ok(AgentTurnCommit {
            events,
            next_assignments,
        })
    }

    /// Atomically records one provider failure, restores inflight input, and advances the floor.
    ///
    /// # Errors
    ///
    /// Returns an exact active-turn conflict or storage failure.
    pub async fn fail_agent_turn(
        &self,
        room_id: &str,
        session_id: &str,
        turn_id: &str,
        error_code: &str,
        message: &str,
        confirmed_runtime_stop: Option<(&str, &str, &str)>,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, room_id).await?;
        let mut session = load_session(&mut transaction, room_id, session_id).await?;
        require_active_turn(&session, turn_id)?;
        if let Some((handle_id, owner_id, lease_token)) = confirmed_runtime_stop {
            if handle_id.is_empty()
                || owner_id.is_empty()
                || lease_token.is_empty()
                || session.runtime_handle_id != handle_id
                || session.runtime_owner_id != owner_id
                || session.runtime_lease_token != lease_token
            {
                return Err(rejected(
                    "stale_provider_turn",
                    "Confirmed provider shutdown does not match durable turn authority.",
                ));
            }
            session.runtime_handle_id.clear();
            session.runtime_owner_id.clear();
            session.runtime_lease_token.clear();
            session.public.provider_session_active = false;
            session.public.provider_session_reused = false;
        }
        let code = public_error_code(error_code);
        let message = clean_message(&redact_persisted_diagnostic_text(message, 512), 512);
        let message = if has_visible_text(&message) {
            message
        } else {
            "Provider turn failed.".to_owned()
        };
        let error = error_event(&mut transaction, &session, turn_id, code, &message).await?;
        let finished =
            turn_finished_event(&mut transaction, &session, turn_id, "error", None, None).await?;
        session.pending_inputs = merge_room_inputs(
            session
                .inflight_inputs
                .iter()
                .chain(&session.pending_inputs),
        )
        .map_err(|_| {
            rejected(
                "stored_turn_authority_invalid",
                "Stored Agent Session turn queue authority is inconsistent or oversized.",
            )
        })?;
        session.inflight_inputs.clear();
        "error".clone_into(&mut session.public.status);
        "error".clone_into(&mut session.public.runtime_status);
        session.public.turn_phase.clear();
        session.public.active_turn_id.clear();
        session.public.last_error = message;
        session.public.last_error_code = code.to_owned();
        session.public.recovery_required = true;
        clear_active_turn_fields(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let state = session_state_event(&mut transaction, &session).await?;
        let prepared = assign_available_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![error, finished, state];
        let mut next_assignments = Vec::with_capacity(prepared.len());
        for item in prepared {
            events.extend(item.events);
            next_assignments.push(item.assignment);
        }
        transaction.commit().await?;
        Ok(AgentTurnCommit {
            events,
            next_assignments,
        })
    }
}

fn public_assignment_error_code(value: &str) -> &'static str {
    match value {
        "agent_session_capacity" => "agent_session_capacity",
        "provider_sync_cursor_mismatch" => "provider_sync_cursor_mismatch",
        "queued_room_event_invalid" => "queued_room_event_invalid",
        "room_event_missing" => "room_event_missing",
        "provider_turn_input_invalid" => "provider_turn_input_invalid",
        "stored_turn_authority_invalid" => "stored_turn_authority_invalid",
        _ => "internal_assignment_error",
    }
}

fn apply_provider_session_transition(
    session: &mut DurableAgentSession,
    provider_session_id: Option<&str>,
) -> Result<(), PersistenceError> {
    let Some(next) = provider_session_id else {
        return Ok(());
    };
    if next.is_empty()
        || next.len() > 200
        || next.trim() != next
        || next.chars().any(char::is_control)
        || next.starts_with("pending-antigravity-")
        || (next != session.provider_session_id
            && (session.public.provider_kind != "antigravity_live_session"
                || !session
                    .provider_session_id
                    .starts_with("pending-antigravity-")))
    {
        return Err(rejected(
            "provider_session_invalid",
            "The provider session transition is invalid.",
        ));
    }
    next.clone_into(&mut session.provider_session_id);
    session.public.provider_session_active = true;
    Ok(())
}

fn complete_session_state(session: &mut DurableAgentSession, input_event_id: &str, input_seq: i64) {
    "attached".clone_into(&mut session.public.status);
    "idle".clone_into(&mut session.public.runtime_status);
    session.public.turn_phase.clear();
    session.public.active_turn_id.clear();
    input_event_id.clone_into(&mut session.public.last_seen_event_id);
    session.public.last_seen_seq = input_seq;
    input_event_id.clone_into(&mut session.public.last_provider_sync_event_id);
    session.public.last_provider_sync_seq = input_seq;
    if session.public.turn_count == 0 {
        session.public.bootstrap_cutoff_seq = input_seq;
    }
    session.public.turn_count = session.public.turn_count.saturating_add(1);
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session.inflight_inputs.clear();
    clear_active_turn_fields(session);
    session.public.updated_at = Utc::now();
}

async fn validate_publication_target(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session: &DurableAgentSession,
    target_agent_id: &str,
) -> Result<(), PersistenceError> {
    if target_agent_id.is_empty() {
        return Ok(());
    }
    if target_agent_id == session.public.session_id
        || target_agent_id.len() > 128
        || target_agent_id.trim() != target_agent_id
        || target_agent_id.chars().any(char::is_control)
    {
        return Err(rejected(
            "room_portal_publication_invalid",
            "The RoomPortal handoff target is invalid.",
        ));
    }
    let target = match load_session(transaction, &session.public.room_id, target_agent_id).await {
        Ok(target) => target,
        Err(PersistenceError::CommandRejected {
            code: "not_found", ..
        }) => {
            return Err(rejected(
                "room_portal_publication_invalid",
                "The RoomPortal handoff target does not exist.",
            ));
        }
        Err(error) => return Err(error),
    };
    let participant = load_participant(
        transaction,
        &session.public.room_id,
        &target.public.participant_id,
    )
    .await?;
    if participant.status == agentsassemble_domain::ParticipantStatus::Kicked || participant.muted {
        return Err(rejected(
            "room_portal_publication_invalid",
            "The RoomPortal handoff target cannot receive the ordered floor.",
        ));
    }
    Ok(())
}

#[path = "room_turn_context.rs"]
mod context;
#[path = "room_turn_routing.rs"]
mod routing;
#[path = "room_turn_scheduler.rs"]
mod scheduler;
#[path = "room_turn_support.rs"]
pub(crate) mod support;

use scheduler::{assign_available_pending, route_message};
use support::{
    agent_final_event, clear_active_turn_fields, error_event, insert_event, load_active_room,
    load_participant, next_sequence, public_error_code, rejected, rejection, require_active_turn,
    session_state_event, turn_finished_event, validate_identifier, validate_input_cursor,
};

#[cfg(test)]
#[path = "room_turn_tests.rs"]
mod tests;
