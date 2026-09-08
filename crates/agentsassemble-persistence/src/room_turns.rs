use agentsassemble_domain::{
    Actor, AgentRuntimeStatus, AgentSessionStatus, AgentTurnPhase, AuthenticatedPrincipal,
    DurableAgentSession, MessageSend, Participant, RoomEvent, RoomInputDeliveryKind, VoteCommand,
    canonical_payload_hash, prepare_message_event,
};
use chrono::Utc;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, RoomMutationAuthority, SqliteStore,
    agent_lifecycle::load_session,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    message_attachments::{bind_message_attachments, prepare_message_attachment_bindings},
    room_event_sequence::next_sequence,
    room_write_budget::command_size,
};

#[derive(Debug, Clone)]
pub struct AgentTurnAssignment {
    pub session: DurableAgentSession,
    pub turn_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub delivery_kind: RoomInputDeliveryKind,
    pub provider_input: String,
    pub room_view: String,
    pub attachment_ids: Vec<String>,
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
    pub room_id: &'a str,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub turn_generation: u64,
    pub execution_id: &'a str,
    pub start_dispatch_nonce: &'a str,
    pub runtime_handle_id: &'a str,
    pub runtime_owner_id: &'a str,
    pub runtime_lease_token: &'a str,
    pub provider_turn_id: &'a str,
    pub provider_session_id: Option<&'a str>,
}

pub(super) struct PreparedAssignment {
    assignment: AgentTurnAssignment,
    events: Vec<RoomEvent>,
}

pub(crate) async fn assign_pending_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room: &agentsassemble_domain::Room,
    settings: &agentsassemble_domain::RoomSettings,
) -> Result<AgentTurnCommit, PersistenceError> {
    let prepared = assign_available_pending(transaction, room, settings).await?;
    let mut events = Vec::new();
    let mut next_assignments = Vec::with_capacity(prepared.len());
    for item in prepared {
        events.extend(item.events);
        next_assignments.push(item.assignment);
    }
    Ok(AgentTurnCommit {
        events,
        next_assignments,
    })
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
        let commit = assign_pending_in(&mut transaction, &room, &settings).await?;
        if commit.next_assignments.is_empty() {
            transaction.commit().await?;
            return Ok(None);
        }
        transaction.commit().await?;
        Ok(Some(commit))
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
        let mut transaction = self.pool.begin().await?;
        let mutation =
            execute_message_in(&mut transaction, principal, request_id, action, payload).await?;
        transaction.commit().await?;
        Ok(mutation)
    }

    /// Commits one room-session message in the transaction that revalidates its exact session.
    ///
    /// # Errors
    ///
    /// Returns session provenance, idempotency, room-state, or storage failures.
    pub async fn execute_authorized_message_with_turn(
        &self,
        authorization: RoomMutationAuthority<'_>,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let current = authorization.resolve(&mut transaction).await?;
        let mutation =
            execute_message_in(&mut transaction, &current, request_id, action, payload).await?;
        transaction.commit().await?;
        Ok(mutation)
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
        let mut transaction = self.pool.begin().await?;
        let commit = completion::complete_message(
            &mut transaction,
            room_id,
            session_id,
            authority,
            content,
            target_agent_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(commit)
    }

    /// Atomically applies one provider `RoomPortal` vote and completes its exact active turn.
    ///
    /// # Errors
    ///
    /// Returns an exact active-turn, participant, vote, or storage failure without a partial event.
    pub async fn complete_agent_vote_turn(
        &self,
        room_id: &str,
        session_id: &str,
        authority: ProviderTurnAuthority<'_>,
        command: VoteCommand,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let commit =
            completion::complete_vote(&mut transaction, room_id, session_id, authority, command)
                .await?;
        transaction.commit().await?;
        Ok(commit)
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
        let mut transaction = self.pool.begin().await?;
        let commit = completion::decline(
            &mut transaction,
            room_id,
            session_id,
            authority,
            reason_code,
        )
        .await?;
        transaction.commit().await?;
        Ok(commit)
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
        authority: ProviderTurnAuthority<'_>,
        error_code: &str,
        message: &str,
        confirmed_runtime_stop: Option<(&str, &str, &str)>,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let commit = completion::fail(
            &mut transaction,
            room_id,
            session_id,
            authority,
            error_code,
            message,
            confirmed_runtime_stop,
        )
        .await?;
        transaction.commit().await?;
        Ok(commit)
    }
}

async fn execute_message_in(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
    payload: &Value,
) -> Result<RoomCommandMutation, PersistenceError> {
    let payload_hash = canonical_payload_hash(payload);
    let (room, settings) = load_active_room(transaction, &principal.room_id).await?;
    if let Some(outcome) = admit_non_lifecycle_command(
        transaction,
        &principal.room_id,
        &principal.principal_id,
        request_id,
        action,
        &payload_hash,
        command_size(request_id, action, payload)?,
    )
    .await?
    {
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
    let participant =
        load_participant(transaction, &principal.room_id, &principal.participant_id).await?;
    let sequence = next_sequence(transaction, &principal.room_id).await?;
    let now = Utc::now();
    let (event, route_to_floor) =
        prepare_human_message_event(transaction, principal, &participant, payload, sequence, now)
            .await?;
    let prepared = if route_to_floor {
        route_message(transaction, &settings, &event).await?;
        assign_available_pending(transaction, &room, &settings).await?
    } else {
        Vec::new()
    };
    let mut events = vec![event.clone()];
    let mut assignments = Vec::with_capacity(prepared.len());
    for item in prepared {
        events.extend(item.events);
        assignments.push(item.assignment);
    }
    let result = json!({"event": event, "event_seq": sequence});
    store_command_result(
        transaction,
        (&principal.room_id, &principal.principal_id),
        request_id,
        action,
        &payload_hash,
        &result,
    )
    .await?;
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

async fn prepare_human_message_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    payload: &Value,
    sequence: i64,
    now: chrono::DateTime<Utc>,
) -> Result<(RoomEvent, bool), PersistenceError> {
    if payload.get("kind").is_some() {
        let command = VoteCommand::from_payload(payload).map_err(rejection)?;
        let route_to_floor = matches!(command, VoteCommand::Create(_));
        let event = crate::room_votes::apply_vote_command(
            transaction,
            principal,
            participant,
            command,
            sequence,
            now,
        )
        .await?;
        return Ok((event, route_to_floor));
    }
    let command = MessageSend::from_payload(payload).map_err(rejection)?;
    let attachments = prepare_message_attachment_bindings(
        transaction,
        principal,
        &command.attachment_ids,
        now.timestamp(),
    )
    .await?;
    let mut event = prepare_message_event(principal, participant, &command, sequence, now)
        .map_err(rejection)?;
    if !attachments.is_empty() {
        event
            .extra
            .insert("attachments".to_owned(), serde_json::to_value(attachments)?);
    }
    insert_event(transaction, &event).await?;
    bind_message_attachments(
        transaction,
        principal,
        &command.attachment_ids,
        sequence,
        now.timestamp(),
    )
    .await?;
    Ok((event, true))
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
        || next != session.provider_session_id
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
    session.public.status = AgentSessionStatus::Attached;
    session.public.runtime_status = AgentRuntimeStatus::Idle;
    session.public.turn_phase = AgentTurnPhase::None;
    session.public.active_turn_id.clear();
    input_event_id.clone_into(&mut session.public.last_seen_event_id);
    session.public.last_seen_seq = input_seq;
    input_event_id.clone_into(&mut session.public.last_provider_sync_event_id);
    session.public.last_provider_sync_seq = input_seq;
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

#[path = "room_turn_completion.rs"]
mod completion;
#[path = "room_turn_context.rs"]
mod context;
#[path = "room_turn_finalization.rs"]
mod finalization;
#[path = "room_turn_routing.rs"]
mod routing;
#[path = "room_turn_scheduler.rs"]
mod scheduler;
#[path = "room_turn_support.rs"]
pub(crate) mod support;

pub(crate) use scheduler::remove_pending_input_reference;
use scheduler::{assign_available_pending, route_message};
use support::{
    clear_active_turn_fields, error_event, insert_event, load_active_room, load_participant,
    rejected, rejection, session_state_event, turn_finished_event,
};

#[cfg(test)]
#[path = "room_turn_tests.rs"]
mod tests;

#[path = "attendee_turn_report.rs"]
mod attendee_report;
pub use attendee_report::{AttendeeTurnOutcome, AttendeeTurnReport};
