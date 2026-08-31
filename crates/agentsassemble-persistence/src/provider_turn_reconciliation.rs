use std::collections::HashSet;

use agentsassemble_domain::{DurableAgentSession, ParticipantStatus, has_visible_text};
use chrono::{SecondsFormat, Utc};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    AgentTurnAssignment, AgentTurnCommit, PersistenceError, ProviderTurnExecution,
    ProviderTurnExecutionPhase, ProviderTurnInterruptCause, ProviderTurnInterruptEffect,
    SqliteStore,
    agent_lifecycle::{load_session, save_session},
    agent_reconciliation::load_candidate as load_lifecycle_candidate,
    message_attachments::message_attachment_ids_from_events,
    provider_turn_effect::{load_optional_effect_in, require_exact_effect},
    provider_turn_execution::load_execution_in,
    room_turns::assign_pending_in,
    room_turns::support::{
        clear_active_turn_fields, error_event, load_active_room, load_participant,
        session_state_event, turn_finished_event,
    },
    turn_authority::active_turn_authority,
    turn_queue::merge_room_inputs,
};

const SCAN_LIMIT: i64 = 64;
const MAX_PROVIDER_INPUT_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
const MAX_ROOM_AGENT_IDS: usize = 64;
const MAX_AUTHORITY_ID_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnReconciliationCursor {
    room_id: String,
    session_id: String,
    turn_generation: u64,
}

#[derive(Debug, Clone)]
pub struct ProviderTurnReconciliationCandidate {
    pub session: DurableAgentSession,
    pub execution: ProviderTurnExecution,
    pub effect: Option<ProviderTurnInterruptEffect>,
}

#[derive(Debug)]
pub struct ProviderTurnReconciliationPage {
    pub candidates: Vec<ProviderTurnReconciliationCandidate>,
    pub next_cursor: Option<ProviderTurnReconciliationCursor>,
}

impl SqliteStore {
    /// Loads the one blocking provider turn that owns a session's current runtime authority.
    ///
    /// # Errors
    ///
    /// Returns malformed or conflicting durable provider-turn authority as a failure.
    pub async fn load_active_provider_turn_reconciliation_candidate(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Option<ProviderTurnReconciliationCandidate>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let generations = sqlx::query_scalar::<_, i64>(
            "SELECT turn_generation FROM provider_turn_executions \
             WHERE room_id = ? AND session_id = ? \
             AND phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', \
               'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required') \
             ORDER BY turn_generation DESC LIMIT 2",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_all(&mut *transaction)
        .await?;
        let Some(generation) = generations.first().copied() else {
            transaction.commit().await?;
            return Ok(None);
        };
        if generations.len() != 1 {
            return Err(invalid_reconciliation());
        }
        let generation = u64::try_from(generation).map_err(|_| invalid_reconciliation())?;
        let execution =
            load_execution_in(&mut transaction, room_id, session_id, generation).await?;
        let session = load_session(&mut transaction, room_id, session_id).await?;
        validate_candidate(&session, &execution)?;
        let effect =
            load_optional_effect_in(&mut transaction, room_id, session_id, generation).await?;
        transaction.commit().await?;
        Ok(Some(ProviderTurnReconciliationCandidate {
            session,
            execution,
            effect,
        }))
    }

    /// Scans one bounded page of blocking provider-turn executions before admission.
    ///
    /// # Errors
    ///
    /// Returns malformed stored authority or storage failures.
    pub async fn load_provider_turn_reconciliation_page(
        &self,
        cursor: Option<&ProviderTurnReconciliationCursor>,
    ) -> Result<ProviderTurnReconciliationPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let cursor_room = cursor.map(|cursor| cursor.room_id.as_str());
        let cursor_session = cursor.map(|cursor| cursor.session_id.as_str());
        let cursor_generation = cursor
            .map(|cursor| generation_i64(cursor.turn_generation))
            .transpose()?;
        let rows = sqlx::query(
            "SELECT room_id, session_id, turn_generation FROM provider_turn_executions \
             WHERE phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', \
               'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required') \
             AND (? IS NULL OR room_id > ? OR (room_id = ? AND session_id > ?) \
               OR (room_id = ? AND session_id = ? AND turn_generation > ?)) \
             ORDER BY room_id, session_id, turn_generation LIMIT ?",
        )
        .bind(cursor_room)
        .bind(cursor_room)
        .bind(cursor_room)
        .bind(cursor_session)
        .bind(cursor_room)
        .bind(cursor_session)
        .bind(cursor_generation)
        .bind(SCAN_LIMIT)
        .fetch_all(&mut *transaction)
        .await?;
        let next_cursor = if rows.len() == usize::try_from(SCAN_LIMIT).unwrap_or(64) {
            rows.last()
                .map(
                    |row| -> Result<ProviderTurnReconciliationCursor, PersistenceError> {
                        let generation = row.get::<i64, _>("turn_generation");
                        Ok(ProviderTurnReconciliationCursor {
                            room_id: row.get("room_id"),
                            session_id: row.get("session_id"),
                            turn_generation: u64::try_from(generation)
                                .map_err(|_| invalid_reconciliation())?,
                        })
                    },
                )
                .transpose()?
        } else {
            None
        };
        let mut candidates = Vec::with_capacity(rows.len());
        for row in rows {
            let room_id = row.get::<String, _>("room_id");
            let session_id = row.get::<String, _>("session_id");
            let generation = u64::try_from(row.get::<i64, _>("turn_generation"))
                .map_err(|_| invalid_reconciliation())?;
            let execution =
                load_execution_in(&mut transaction, &room_id, &session_id, generation).await?;
            let session = load_session(&mut transaction, &room_id, &session_id).await?;
            validate_candidate(&session, &execution)?;
            let effect =
                load_optional_effect_in(&mut transaction, &room_id, &session_id, generation)
                    .await?;
            candidates.push(ProviderTurnReconciliationCandidate {
                session,
                execution,
                effect,
            });
        }
        transaction.commit().await?;
        Ok(ProviderTurnReconciliationPage {
            candidates,
            next_cursor,
        })
    }

    /// Loads one exact blocking provider-turn candidate for a known durable generation.
    ///
    /// # Errors
    ///
    /// Returns missing, malformed, terminal, or changed stored authority as a failure.
    pub async fn load_provider_turn_reconciliation_candidate(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
    ) -> Result<ProviderTurnReconciliationCandidate, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let execution =
            load_execution_in(&mut transaction, room_id, session_id, turn_generation).await?;
        let session = load_session(&mut transaction, room_id, session_id).await?;
        validate_candidate(&session, &execution)?;
        let effect =
            load_optional_effect_in(&mut transaction, room_id, session_id, turn_generation).await?;
        transaction.commit().await?;
        Ok(ProviderTurnReconciliationCandidate {
            session,
            execution,
            effect,
        })
    }

    /// Rehydrates only a proved pre-dispatch assignment from its immutable envelope.
    ///
    /// # Errors
    ///
    /// Rejects changed, muted, effect-bearing, dispatched, or malformed authority.
    pub async fn recover_assigned_provider_turn(
        &self,
        candidate: &ProviderTurnReconciliationCandidate,
    ) -> Result<AgentTurnAssignment, PersistenceError> {
        let expected = &candidate.execution;
        if expected.phase != ProviderTurnExecutionPhase::Assigned
            || candidate.effect.is_some()
            || !expected.start_dispatch_nonce.is_empty()
            || !expected.provider_turn_id.is_empty()
            || expected.requeue_finalized
        {
            return Err(stale_reconciliation());
        }
        let mut transaction = self.pool.begin().await?;
        let session =
            load_session(&mut transaction, &expected.room_id, &expected.session_id).await?;
        let execution = load_execution_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        validate_candidate(&session, &execution)?;
        if session != candidate.session || &execution != expected {
            return Err(stale_reconciliation());
        }
        if load_optional_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?
        .is_some()
        {
            return Err(stale_reconciliation());
        }
        let participant = load_participant(
            &mut transaction,
            &expected.room_id,
            &expected.participant_id,
        )
        .await?;
        if participant.status != ParticipantStatus::Joined || participant.muted {
            return Err(stale_reconciliation());
        }
        let assignment_json = sqlx::query_scalar::<_, String>(
            "SELECT assignment_json FROM provider_turn_executions \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND phase = 'assigned'",
        )
        .bind(&expected.room_id)
        .bind(&expected.session_id)
        .bind(generation_i64(expected.turn_generation)?)
        .bind(&expected.execution_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or_else(stale_reconciliation)?;
        let assignment = serde_json::from_str::<
            crate::provider_turn_execution::ProviderTurnAssignmentEnvelope,
        >(&assignment_json)?;
        validate_assignment_envelope(&mut transaction, &session, &assignment).await?;
        transaction.commit().await?;
        Ok(AgentTurnAssignment {
            session,
            turn_id: execution.turn_id,
            turn_generation: execution.turn_generation,
            execution_id: execution.execution_id,
            delivery_kind: assignment.delivery_kind,
            provider_input: assignment.provider_input,
            room_view: assignment.room_view,
            attachment_ids: assignment.attachment_ids,
            room_agent_ids: assignment.room_agent_ids,
            tabletop_tools: assignment.tabletop_tools,
        })
    }

    /// Terminalizes an exact assigned or start-authorized turn after retained no-I/O proof.
    ///
    /// # Errors
    ///
    /// Rejects any changed execution, runtime authority, or interrupt effect.
    pub async fn finalize_provider_turn_not_started(
        &self,
        candidate: &ProviderTurnReconciliationCandidate,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let expected = &candidate.execution;
        if candidate.effect.is_some()
            || !matches!(
                expected.phase,
                ProviderTurnExecutionPhase::Assigned | ProviderTurnExecutionPhase::StartDispatching
            )
        {
            return Err(stale_reconciliation());
        }
        let mut transaction = self.pool.begin().await?;
        let session =
            load_session(&mut transaction, &expected.room_id, &expected.session_id).await?;
        let execution = load_execution_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        validate_candidate(&session, &execution)?;
        if expected != &execution
            || load_optional_effect_in(
                &mut transaction,
                &expected.room_id,
                &expected.session_id,
                expected.turn_generation,
            )
            .await?
            .is_some()
        {
            return Err(stale_reconciliation());
        }
        crate::provider_turn_execution::finalize_proven_no_effect_task_death(
            transaction,
            session,
            &execution,
        )
        .await
    }

    /// Finalizes a blocking provider turn only after positive exact runtime-gone proof.
    ///
    /// # Errors
    ///
    /// Rejects any changed session, execution, effect, or H/O/T custody value.
    pub async fn finalize_provider_turn_runtime_gone(
        &self,
        candidate: &ProviderTurnReconciliationCandidate,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        self.finalize_provider_turn_runtime_gone_with(candidate, FloorProgression::Assign)
            .await
    }

    /// Finalizes exact runtime-gone authority while the complete runtime is shutting down.
    /// Pending inputs remain durable, but no stopped provider receives a new assignment.
    ///
    /// # Errors
    ///
    /// Rejects any changed session, execution, effect, or H/O/T custody value.
    pub async fn finalize_provider_turn_runtime_gone_for_shutdown(
        &self,
        candidate: &ProviderTurnReconciliationCandidate,
    ) -> Result<(), PersistenceError> {
        self.finalize_provider_turn_runtime_gone_with(candidate, FloorProgression::Defer)
            .await
            .map(|_| ())
    }

    async fn finalize_provider_turn_runtime_gone_with(
        &self,
        candidate: &ProviderTurnReconciliationCandidate,
        floor_progression: FloorProgression,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let expected = &candidate.execution;
        let mut transaction = self.pool.begin().await?;
        let session =
            load_session(&mut transaction, &expected.room_id, &expected.session_id).await?;
        let execution = load_execution_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        if expected != &execution {
            return Err(stale_reconciliation());
        }
        validate_candidate(&session, &execution)?;
        let current_effect = load_optional_effect_in(
            &mut transaction,
            &expected.room_id,
            &expected.session_id,
            expected.turn_generation,
        )
        .await?;
        match (&candidate.effect, &current_effect) {
            (Some(expected_effect), Some(current_effect)) => {
                require_exact_effect(expected_effect, current_effect)?;
                if current_effect.phase == crate::ProviderTurnEffectPhase::Finalized {
                    return Err(stale_reconciliation());
                }
            }
            (None, None) => {}
            _ => return Err(stale_reconciliation()),
        }
        if stop_effect_is_confirmed_by_runtime_gone(&mut transaction, &session).await? {
            crate::provider_turn_stop::terminalize_confirmed_stop_turn(&mut transaction, &session)
                .await?;
            let mut session = session;
            "effect_applied".clone_into(&mut session.lifecycle_intent_status);
            session.public.updated_at = Utc::now();
            save_session(&mut transaction, &session).await?;
            transaction.commit().await?;
            return Ok(AgentTurnCommit {
                events: Vec::new(),
                next_assignments: Vec::new(),
            });
        }
        let terminal_phase = if current_effect.is_some() {
            ProviderTurnExecutionPhase::Interrupted
        } else {
            ProviderTurnExecutionPhase::Failed
        };
        let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
        terminalize_runtime_gone_authority(
            &mut transaction,
            &execution,
            current_effect.as_ref(),
            terminal_phase,
            &now,
        )
        .await?;
        let interrupt_cause = candidate.effect.as_ref().map(|effect| effect.cause);
        finalize_runtime_gone_session(
            transaction,
            session,
            &execution,
            interrupt_cause,
            floor_progression,
        )
        .await
    }
}

async fn stop_effect_is_confirmed_by_runtime_gone(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    if session.lifecycle_intent_action != "stop"
        || !matches!(
            session.lifecycle_intent_status.as_str(),
            "effect_inflight" | "unconfirmed" | "effect_applied"
        )
    {
        return Ok(false);
    }
    let lifecycle = load_lifecycle_candidate(
        transaction,
        &session.public.room_id,
        &session.public.session_id,
    )
    .await?
    .ok_or_else(invalid_reconciliation)?;
    let reservation = lifecycle
        .reservation
        .as_ref()
        .ok_or_else(invalid_reconciliation)?;
    if lifecycle.session != *session
        || reservation.action != "agent.stop"
        || reservation.operation_id != session.lifecycle_intent_id
    {
        return Err(invalid_reconciliation());
    }
    Ok(true)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FloorProgression {
    Assign,
    Defer,
}

async fn terminalize_runtime_gone_authority(
    transaction: &mut Transaction<'_, Sqlite>,
    execution: &ProviderTurnExecution,
    effect: Option<&ProviderTurnInterruptEffect>,
    terminal_phase: ProviderTurnExecutionPhase,
    now: &str,
) -> Result<(), PersistenceError> {
    let execution_changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = ?, requeue_finalized = 1, updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND phase = ? AND requeue_finalized = 0 \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(terminal_phase.as_str())
    .bind(now)
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(&execution.execution_id)
    .bind(execution.phase.as_str())
    .bind(&execution.runtime_handle_id)
    .bind(&execution.runtime_owner_id)
    .bind(&execution.runtime_lease_token)
    .execute(&mut **transaction)
    .await?;
    if execution_changed.rows_affected() != 1 {
        return Err(stale_reconciliation());
    }
    if let Some(effect) = effect {
        let effect_changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'finalized', claim_expires_at = NULL, \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = ? \
             AND dispatch_nonce = ?",
        )
        .bind(now)
        .bind(&effect.room_id)
        .bind(&effect.effect_id)
        .bind(effect.phase.as_str())
        .bind(&effect.dispatch_nonce)
        .execute(&mut **transaction)
        .await?;
        if effect_changed.rows_affected() != 1 {
            return Err(stale_reconciliation());
        }
    }
    Ok(())
}

async fn finalize_runtime_gone_session(
    mut transaction: Transaction<'_, Sqlite>,
    mut session: DurableAgentSession,
    execution: &ProviderTurnExecution,
    interrupt_cause: Option<ProviderTurnInterruptCause>,
    floor_progression: FloorProgression,
) -> Result<AgentTurnCommit, PersistenceError> {
    let mut events = Vec::with_capacity(3);
    if interrupt_cause.is_none() {
        events.push(
            error_event(
                &mut transaction,
                &session,
                &execution.turn_id,
                "provider_runtime_gone",
                "The provider runtime exited before its exact turn could be recovered.",
            )
            .await?,
        );
    } else if interrupt_cause == Some(ProviderTurnInterruptCause::AgentInterrupt) {
        events.push(
            error_event(
                &mut transaction,
                &session,
                &execution.turn_id,
                "interrupted",
                "The provider turn was interrupted by a room operator.",
            )
            .await?,
        );
    }
    events.push(
        turn_finished_event(
            &mut transaction,
            &session,
            &execution.turn_id,
            if interrupt_cause.is_some() {
                "interrupted"
            } else {
                "error"
            },
            None,
            Some(
                interrupt_cause.map_or("provider_runtime_gone", ProviderTurnInterruptCause::as_str),
            ),
        )
        .await?,
    );
    session.pending_inputs = merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| invalid_reconciliation())?;
    session.inflight_inputs.clear();
    "detached".clone_into(&mut session.public.status);
    "stopped".clone_into(&mut session.public.runtime_status);
    session.public.enabled = false;
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.provider_session_id.clear();
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.runtime_lease_token.clear();
    session.schedule_requested = false;
    if interrupt_cause == Some(ProviderTurnInterruptCause::AgentInterrupt) {
        "The provider turn was interrupted by a room operator."
            .clone_into(&mut session.public.last_error);
        "interrupted".clone_into(&mut session.public.last_error_code);
    }
    session.public.recovery_required = false;
    clear_active_turn_fields(&mut session);
    session.public.updated_at = Utc::now();
    save_session(&mut transaction, &session).await?;
    let mut participant = load_participant(
        &mut transaction,
        &session.public.room_id,
        &session.public.participant_id,
    )
    .await?;
    participant.status = ParticipantStatus::Detached;
    participant.updated_at = Utc::now();
    save_participant(&mut transaction, &participant).await?;
    events.push(session_state_event(&mut transaction, &session).await?);
    let scheduled = if floor_progression == FloorProgression::Assign
        && interrupt_cause != Some(ProviderTurnInterruptCause::AgentInterrupt)
    {
        let (room, settings) = load_active_room(&mut transaction, &execution.room_id).await?;
        assign_pending_in(&mut transaction, &room, &settings).await?
    } else {
        AgentTurnCommit {
            events: Vec::new(),
            next_assignments: Vec::new(),
        }
    };
    transaction.commit().await?;
    events.extend(scheduled.events);
    Ok(AgentTurnCommit {
        events,
        next_assignments: scheduled.next_assignments,
    })
}

async fn validate_assignment_envelope(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    assignment: &crate::provider_turn_execution::ProviderTurnAssignmentEnvelope,
) -> Result<(), PersistenceError> {
    let mut inflight_events = Vec::with_capacity(session.inflight_inputs.len());
    for input in &session.inflight_inputs {
        inflight_events.push(
            crate::room_turns::support::load_event(
                transaction,
                &session.public.room_id,
                &input.event_id,
            )
            .await?
            .ok_or_else(invalid_reconciliation)?,
        );
    }
    let expected_attachment_ids = message_attachment_ids_from_events(inflight_events.iter())
        .map_err(|_| invalid_reconciliation())?;
    let unique_agent_ids = assignment.room_agent_ids.iter().collect::<HashSet<_>>();
    if session.input_up_to_seq <= 0
        || session.active_source_event_id.is_empty()
        || session.input_up_to_event_id != session.active_source_event_id
        || session.inflight_inputs.is_empty()
        || session
            .inflight_inputs
            .iter()
            .any(|input| input.delivery_kind != assignment.delivery_kind)
        || assignment.provider_input.chars().count() > MAX_PROVIDER_INPUT_CHARS
        || assignment.provider_input.contains('\0')
        || !has_visible_text(&assignment.provider_input)
        || assignment.room_view.chars().count() > MAX_ROOM_VIEW_CHARS
        || assignment.room_view.len() > MAX_ROOM_VIEW_BYTES
        || assignment.room_view.contains('\0')
        || !has_visible_text(&assignment.room_view)
        || assignment.attachment_ids != expected_attachment_ids
        || assignment.room_agent_ids.len() > MAX_ROOM_AGENT_IDS
        || unique_agent_ids.len() != assignment.room_agent_ids.len()
        || assignment.room_agent_ids.iter().any(|agent_id| {
            agent_id.is_empty()
                || agent_id.len() > MAX_AUTHORITY_ID_BYTES
                || agent_id.trim() != agent_id
                || agent_id.chars().any(char::is_control)
        })
    {
        return Err(invalid_reconciliation());
    }
    Ok(())
}

fn validate_candidate(
    session: &DurableAgentSession,
    execution: &ProviderTurnExecution,
) -> Result<(), PersistenceError> {
    if !execution.phase.is_blocking()
        || !active_turn_authority(session).map_err(|_| invalid_reconciliation())?
        || session.public.room_id != execution.room_id
        || session.public.session_id != execution.session_id
        || session.public.participant_id != execution.participant_id
        || session.public.active_turn_id != execution.turn_id
        || session.turn_generation != execution.turn_generation
        || session.runtime_handle_id != execution.runtime_handle_id
        || session.runtime_owner_id != execution.runtime_owner_id
        || session.runtime_lease_token != execution.runtime_lease_token
    {
        return Err(invalid_reconciliation());
    }
    Ok(())
}

async fn save_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &agentsassemble_domain::Participant,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(participant)?)
    .bind(&participant.room_id)
    .bind(&participant.participant_id)
    .execute(&mut **transaction)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(stale_reconciliation());
    }
    Ok(())
}

fn generation_i64(generation: u64) -> Result<i64, PersistenceError> {
    i64::try_from(generation).map_err(|_| invalid_reconciliation())
}

fn invalid_reconciliation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stored_turn_reconciliation_invalid",
        message: "Stored provider turn reconciliation authority is invalid.".to_owned(),
    }
}

fn stale_reconciliation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_turn_reconciliation_candidate",
        message: "Provider turn reconciliation authority changed before commit.".to_owned(),
    }
}
