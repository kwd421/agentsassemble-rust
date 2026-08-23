use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, MessageSend, RoomEvent, canonical_payload_hash,
    clean_message, has_visible_text, prepare_message_event, redact_persisted_diagnostic_text,
};
use chrono::Utc;
use serde_json::{Value, json};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle::{load_session, save_session},
    command_admission::admit_non_lifecycle_command,
    turn_queue::merge_event_ids,
};

#[derive(Debug, Clone)]
pub struct AgentTurnAssignment {
    pub session: DurableAgentSession,
    pub turn_id: String,
    pub provider_input: String,
}

#[derive(Debug, Clone)]
pub struct RoomCommandMutation {
    pub outcome: CommandOutcome,
    pub assignment: Option<AgentTurnAssignment>,
}

#[derive(Debug, Clone)]
pub struct AgentTurnCommit {
    pub events: Vec<RoomEvent>,
    pub next_assignment: Option<AgentTurnAssignment>,
}

struct PreparedAssignment {
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
        let Some(prepared) = assign_oldest_pending(&mut transaction, &room, &settings).await?
        else {
            transaction.commit().await?;
            return Ok(None);
        };
        transaction.commit().await?;
        Ok(Some(AgentTurnCommit {
            events: prepared.events,
            next_assignment: Some(prepared.assignment),
        }))
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
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignment: None,
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
        queue_ordered_message(&mut transaction, &settings, &event).await?;
        let prepared = assign_oldest_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![event.clone()];
        let assignment = prepared.map(|prepared| {
            events.extend(prepared.events);
            prepared.assignment
        });
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
            assignment,
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
        turn_id: &str,
        provider_turn_id: &str,
        content: &str,
    ) -> Result<AgentTurnCommit, PersistenceError> {
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
        )
        .await?;
        let finished =
            turn_finished_event(&mut transaction, &session, turn_id, "completed").await?;
        "attached".clone_into(&mut session.public.status);
        "idle".clone_into(&mut session.public.runtime_status);
        session.public.turn_phase.clear();
        session.public.active_turn_id.clear();
        session
            .public
            .last_seen_event_id
            .clone_from(&input_event_id);
        session.public.last_seen_seq = input_seq;
        session
            .public
            .last_provider_sync_event_id
            .clone_from(&input_event_id);
        session.public.last_provider_sync_seq = input_seq;
        if session.public.turn_count == 0 {
            session.public.bootstrap_cutoff_seq = input_seq;
        }
        session.public.turn_count = session.public.turn_count.saturating_add(1);
        session.public.last_error.clear();
        session.public.last_error_code.clear();
        session.public.recovery_required = false;
        session.inflight_event_ids.clear();
        clear_active_turn_fields(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let state = session_state_event(&mut transaction, &session).await?;
        queue_ordered_message(&mut transaction, &settings, &final_event).await?;
        let prepared = assign_oldest_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![final_event, finished, state];
        let next_assignment = prepared.map(|prepared| {
            events.extend(prepared.events);
            prepared.assignment
        });
        transaction.commit().await?;
        Ok(AgentTurnCommit {
            events,
            next_assignment,
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
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, room_id).await?;
        let mut session = load_session(&mut transaction, room_id, session_id).await?;
        require_active_turn(&session, turn_id)?;
        let code = public_error_code(error_code);
        let message = clean_message(&redact_persisted_diagnostic_text(message, 512), 512);
        let message = if has_visible_text(&message) {
            message
        } else {
            "Provider turn failed.".to_owned()
        };
        let error = error_event(&mut transaction, &session, turn_id, code, &message).await?;
        let finished = turn_finished_event(&mut transaction, &session, turn_id, "error").await?;
        session.pending_event_ids = merge_event_ids(
            session
                .inflight_event_ids
                .iter()
                .chain(&session.pending_event_ids),
        )
        .map_err(|_| {
            rejected(
                "stored_turn_authority_invalid",
                "Stored Agent Session turn queue authority is inconsistent or oversized.",
            )
        })?;
        session.inflight_event_ids.clear();
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
        let prepared = assign_oldest_pending(&mut transaction, &room, &settings).await?;
        let mut events = vec![error, finished, state];
        let next_assignment = prepared.map(|prepared| {
            events.extend(prepared.events);
            prepared.assignment
        });
        transaction.commit().await?;
        Ok(AgentTurnCommit {
            events,
            next_assignment,
        })
    }
}

#[path = "room_turn_routing.rs"]
mod routing;
#[path = "room_turn_support.rs"]
mod support;

use support::{
    agent_final_event, assign_oldest_pending, clear_active_turn_fields, error_event, insert_event,
    load_active_room, load_participant, next_sequence, public_error_code, queue_ordered_message,
    rejected, rejection, require_active_turn, session_state_event, turn_finished_event,
    validate_identifier, validate_input_cursor,
};

#[cfg(test)]
#[path = "room_turn_tests.rs"]
mod tests;
