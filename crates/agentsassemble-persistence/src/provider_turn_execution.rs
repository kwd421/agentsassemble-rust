use agentsassemble_domain::{DurableAgentSession, ParticipantStatus, RoomInputDeliveryKind};
use chrono::{SecondsFormat, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::room_turns::{ProviderTurnAuthority, assign_pending_in};
use crate::{
    AgentTurnCommit, PersistenceError, SqliteStore,
    agent_lifecycle::{load_session, save_session},
    room_turns::support::{
        clear_active_turn_fields, error_event, load_active_room, load_participant,
        session_state_event, turn_finished_event,
    },
    turn_authority::active_turn_authority,
    turn_queue::merge_room_inputs,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTurnExecutionPhase {
    Assigned,
    StartDispatching,
    Running,
    InterruptPending,
    Quiescing,
    StartAmbiguous,
    InterruptAmbiguous,
    RecoveryRequired,
    Completed,
    Declined,
    Failed,
    Interrupted,
}

impl ProviderTurnExecutionPhase {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Assigned => "assigned",
            Self::StartDispatching => "start_dispatching",
            Self::Running => "running",
            Self::InterruptPending => "interrupt_pending",
            Self::Quiescing => "quiescing",
            Self::StartAmbiguous => "start_ambiguous",
            Self::InterruptAmbiguous => "interrupt_ambiguous",
            Self::RecoveryRequired => "recovery_required",
            Self::Completed => "completed",
            Self::Declined => "declined",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> Result<Self, PersistenceError> {
        match value {
            "assigned" => Ok(Self::Assigned),
            "start_dispatching" => Ok(Self::StartDispatching),
            "running" => Ok(Self::Running),
            "interrupt_pending" => Ok(Self::InterruptPending),
            "quiescing" => Ok(Self::Quiescing),
            "start_ambiguous" => Ok(Self::StartAmbiguous),
            "interrupt_ambiguous" => Ok(Self::InterruptAmbiguous),
            "recovery_required" => Ok(Self::RecoveryRequired),
            "completed" => Ok(Self::Completed),
            "declined" => Ok(Self::Declined),
            "failed" => Ok(Self::Failed),
            "interrupted" => Ok(Self::Interrupted),
            _ => Err(invalid_execution()),
        }
    }

    #[must_use]
    pub const fn is_blocking(self) -> bool {
        matches!(
            self,
            Self::Assigned
                | Self::StartDispatching
                | Self::Running
                | Self::InterruptPending
                | Self::Quiescing
                | Self::StartAmbiguous
                | Self::InterruptAmbiguous
                | Self::RecoveryRequired
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnExecution {
    pub room_id: String,
    pub session_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub participant_id: String,
    pub turn_id: String,
    pub phase: ProviderTurnExecutionPhase,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub start_dispatch_nonce: String,
    pub provider_turn_id: String,
    pub requeue_finalized: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnStartAuthority {
    pub room_id: String,
    pub session_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub turn_id: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub start_dispatch_nonce: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProviderTurnAssignmentEnvelope {
    pub delivery_kind: RoomInputDeliveryKind,
    pub provider_input: String,
    pub room_view: String,
    pub room_agent_ids: Vec<String>,
    pub tabletop_tools: bool,
}

pub(crate) async fn insert_assigned_execution(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    assignment: &ProviderTurnAssignmentEnvelope,
) -> Result<ProviderTurnExecution, PersistenceError> {
    if session.turn_generation == 0
        || session.public.active_turn_id != turn_id
        || session.runtime_handle_id.is_empty()
        || session.runtime_owner_id.is_empty()
        || session.runtime_lease_token.is_empty()
    {
        return Err(invalid_execution());
    }
    let execution_id = Uuid::new_v4().to_string();
    let generation = generation_i64(session.turn_generation)?;
    let assignment_json = serde_json::to_string(assignment)?;
    let now = canonical_now();
    sqlx::query(
        "INSERT INTO provider_turn_executions(\
         room_id, session_id, turn_generation, execution_id, participant_id, turn_id, assignment_json, phase, \
         runtime_handle_id, runtime_owner_id, runtime_lease_token, created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, 'assigned', ?, ?, ?, ?, ?)",
    )
    .bind(&session.public.room_id)
    .bind(&session.public.session_id)
    .bind(generation)
    .bind(&execution_id)
    .bind(&session.public.participant_id)
    .bind(turn_id)
    .bind(assignment_json)
    .bind(&session.runtime_handle_id)
    .bind(&session.runtime_owner_id)
    .bind(&session.runtime_lease_token)
    .bind(&now)
    .bind(&now)
    .execute(&mut **transaction)
    .await?;
    Ok(ProviderTurnExecution {
        room_id: session.public.room_id.clone(),
        session_id: session.public.session_id.clone(),
        turn_generation: session.turn_generation,
        execution_id,
        participant_id: session.public.participant_id.clone(),
        turn_id: turn_id.to_owned(),
        phase: ProviderTurnExecutionPhase::Assigned,
        runtime_handle_id: session.runtime_handle_id.clone(),
        runtime_owner_id: session.runtime_owner_id.clone(),
        runtime_lease_token: session.runtime_lease_token.clone(),
        start_dispatch_nonce: String::new(),
        provider_turn_id: String::new(),
        requeue_finalized: false,
    })
}

pub(crate) async fn blocking_execution_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM provider_turn_executions \
         WHERE room_id = ? AND session_id = ? \
         AND phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', \
           'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required'))",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_one(&mut **transaction)
    .await?
        != 0)
}

impl SqliteStore {
    /// Consumes the exact durable start authorization before any provider I/O.
    ///
    /// # Errors
    ///
    /// Rejects stale, muted, quarantined, or already consumed assignments.
    pub async fn authorize_provider_turn_start(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
        turn_id: &str,
    ) -> Result<ProviderTurnStartAuthority, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let session = load_session(&mut transaction, room_id, session_id).await?;
        let participant =
            load_participant(&mut transaction, room_id, &session.public.participant_id).await?;
        if participant.room_id != room_id
            || participant.participant_id != session.public.participant_id
            || participant.status != ParticipantStatus::Joined
            || participant.muted
            || session.public.active_turn_id != turn_id
            || session.turn_generation != turn_generation
            || !active_turn_authority(&session).map_err(|_| invalid_execution())?
        {
            return Err(stale_execution());
        }
        let nonce = Uuid::new_v4().to_string();
        let updated = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'start_dispatching', \
             start_dispatch_nonce = ?, updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND turn_id = ? AND participant_id = ? AND phase = 'assigned' \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ? \
             AND NOT EXISTS (SELECT 1 FROM provider_turn_effects effect \
               WHERE effect.room_id = provider_turn_executions.room_id \
               AND effect.session_id = provider_turn_executions.session_id \
               AND effect.turn_generation = provider_turn_executions.turn_generation \
               AND effect.phase != 'finalized')",
        )
        .bind(&nonce)
        .bind(canonical_now())
        .bind(room_id)
        .bind(session_id)
        .bind(generation_i64(turn_generation)?)
        .bind(turn_id)
        .bind(&session.public.participant_id)
        .bind(&session.runtime_handle_id)
        .bind(&session.runtime_owner_id)
        .bind(&session.runtime_lease_token)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(stale_execution());
        }
        let execution =
            load_execution_in(&mut transaction, room_id, session_id, turn_generation).await?;
        transaction.commit().await?;
        Ok(ProviderTurnStartAuthority {
            room_id: execution.room_id,
            session_id: execution.session_id,
            turn_generation: execution.turn_generation,
            execution_id: execution.execution_id,
            turn_id: execution.turn_id,
            runtime_handle_id: execution.runtime_handle_id,
            runtime_owner_id: execution.runtime_owner_id,
            runtime_lease_token: execution.runtime_lease_token,
            start_dispatch_nonce: execution.start_dispatch_nonce,
        })
    }

    /// Marks a started provider turn under the exact dispatch and custody fence.
    ///
    /// # Errors
    ///
    /// Rejects a stale dispatch or malformed provider turn identity.
    pub async fn mark_provider_turn_running(
        &self,
        authority: &ProviderTurnStartAuthority,
        provider_turn_id: &str,
    ) -> Result<(), PersistenceError> {
        if provider_turn_id.is_empty()
            || provider_turn_id.len() > 256
            || provider_turn_id.trim() != provider_turn_id
            || provider_turn_id.chars().any(char::is_control)
        {
            return Err(invalid_execution());
        }
        let updated = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'running', provider_turn_id = ?, \
             updated_at = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND turn_id = ? AND phase = 'start_dispatching' \
             AND start_dispatch_nonce = ? AND runtime_handle_id = ? \
             AND runtime_owner_id = ? AND runtime_lease_token = ?",
        )
        .bind(provider_turn_id)
        .bind(canonical_now())
        .bind(&authority.room_id)
        .bind(&authority.session_id)
        .bind(generation_i64(authority.turn_generation)?)
        .bind(&authority.execution_id)
        .bind(&authority.turn_id)
        .bind(&authority.start_dispatch_nonce)
        .bind(&authority.runtime_handle_id)
        .bind(&authority.runtime_owner_id)
        .bind(&authority.runtime_lease_token)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(stale_execution());
        }
        Ok(())
    }

    /// Quarantines an exact provider turn whose external start result is uncertain.
    ///
    /// # Errors
    ///
    /// Rejects stale dispatch or runtime custody authority.
    pub async fn mark_provider_turn_recovery_required(
        &self,
        authority: &ProviderTurnStartAuthority,
    ) -> Result<(), PersistenceError> {
        let updated = sqlx::query(
            "UPDATE provider_turn_executions SET phase = 'recovery_required', updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND turn_id = ? \
             AND phase IN ('start_dispatching', 'running') AND start_dispatch_nonce = ? \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
        )
        .bind(canonical_now())
        .bind(&authority.room_id)
        .bind(&authority.session_id)
        .bind(generation_i64(authority.turn_generation)?)
        .bind(&authority.execution_id)
        .bind(&authority.turn_id)
        .bind(&authority.start_dispatch_nonce)
        .bind(&authority.runtime_handle_id)
        .bind(&authority.runtime_owner_id)
        .bind(&authority.runtime_lease_token)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(stale_execution());
        }
        Ok(())
    }

    /// Loads one exact durable execution for reconciliation and verification.
    ///
    /// # Errors
    ///
    /// Rejects missing or malformed stored execution authority.
    pub async fn provider_turn_execution(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
    ) -> Result<ProviderTurnExecution, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let execution =
            load_execution_in(&mut transaction, room_id, session_id, turn_generation).await?;
        transaction.commit().await?;
        Ok(execution)
    }

    /// Records a live provider task death under its retained exact assignment identity.
    ///
    /// # Errors
    ///
    /// Rejects stale execution or session authority and never requeues ambiguous I/O.
    pub async fn record_provider_turn_task_death(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let execution =
            load_execution_in(&mut transaction, room_id, session_id, turn_generation).await?;
        let session = load_session(&mut transaction, room_id, session_id).await?;
        if execution.execution_id != execution_id
            || execution.participant_id != session.public.participant_id
            || execution.turn_id != session.public.active_turn_id
            || execution.turn_generation != session.turn_generation
            || execution.runtime_handle_id != session.runtime_handle_id
            || execution.runtime_owner_id != session.runtime_owner_id
            || execution.runtime_lease_token != session.runtime_lease_token
            || !active_turn_authority(&session).map_err(|_| invalid_execution())?
        {
            return Err(stale_execution());
        }
        if execution.phase == ProviderTurnExecutionPhase::Assigned {
            return finalize_proven_no_effect_task_death(transaction, session, &execution).await;
        }
        if matches!(
            execution.phase,
            ProviderTurnExecutionPhase::StartDispatching
                | ProviderTurnExecutionPhase::Running
                | ProviderTurnExecutionPhase::InterruptPending
                | ProviderTurnExecutionPhase::Quiescing
        ) {
            return quarantine_dispatched_task_death(transaction, session, &execution).await;
        }
        Err(stale_execution())
    }
}

pub(crate) async fn finalize_proven_no_effect_task_death(
    mut transaction: Transaction<'_, Sqlite>,
    mut session: DurableAgentSession,
    execution: &ProviderTurnExecution,
) -> Result<AgentTurnCommit, PersistenceError> {
    let (room, settings) = load_active_room(&mut transaction, &execution.room_id).await?;
    let changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'failed', requeue_finalized = 1, \
         updated_at = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
         AND execution_id = ? AND phase = ? AND requeue_finalized = 0 \
         AND NOT EXISTS (SELECT 1 FROM provider_turn_effects effect \
           WHERE effect.room_id = provider_turn_executions.room_id \
           AND effect.session_id = provider_turn_executions.session_id \
           AND effect.turn_generation = provider_turn_executions.turn_generation \
           AND effect.phase != 'finalized')",
    )
    .bind(canonical_now())
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(&execution.execution_id)
    .bind(execution.phase.as_str())
    .execute(&mut *transaction)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(stale_execution());
    }
    let error = error_event(
        &mut transaction,
        &session,
        &execution.turn_id,
        "provider_turn_task_failed",
        "Provider turn ownership ended before provider I/O was authorized.",
    )
    .await?;
    let finished = turn_finished_event(
        &mut transaction,
        &session,
        &execution.turn_id,
        "error",
        None,
        Some("provider_turn_task_failed"),
    )
    .await?;
    session.pending_inputs = merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| invalid_execution())?;
    session.inflight_inputs.clear();
    "error".clone_into(&mut session.public.status);
    "error".clone_into(&mut session.public.runtime_status);
    session.public.turn_phase.clear();
    session.public.active_turn_id.clear();
    "Provider turn ownership ended before provider I/O was authorized."
        .clone_into(&mut session.public.last_error);
    "provider_turn_task_failed".clone_into(&mut session.public.last_error_code);
    session.public.recovery_required = true;
    clear_active_turn_fields(&mut session);
    session.public.updated_at = Utc::now();
    save_session(&mut transaction, &session).await?;
    let state = session_state_event(&mut transaction, &session).await?;
    let scheduled = assign_pending_in(&mut transaction, &room, &settings).await?;
    transaction.commit().await?;
    let mut events = vec![error, finished, state];
    events.extend(scheduled.events);
    Ok(AgentTurnCommit {
        events,
        next_assignments: scheduled.next_assignments,
    })
}

async fn quarantine_dispatched_task_death(
    mut transaction: Transaction<'_, Sqlite>,
    mut session: DurableAgentSession,
    execution: &ProviderTurnExecution,
) -> Result<AgentTurnCommit, PersistenceError> {
    let changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = CASE WHEN EXISTS (\
           SELECT 1 FROM provider_turn_effects effect WHERE effect.room_id = ? \
           AND effect.session_id = ? AND effect.turn_generation = ? \
           AND effect.phase = 'dispatching') THEN 'interrupt_ambiguous' \
           ELSE 'recovery_required' END, updated_at = ? \
         WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
         AND execution_id = ? AND phase = ? AND runtime_handle_id = ? \
         AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(canonical_now())
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(&execution.execution_id)
    .bind(execution.phase.as_str())
    .bind(&execution.runtime_handle_id)
    .bind(&execution.runtime_owner_id)
    .bind(&execution.runtime_lease_token)
    .execute(&mut *transaction)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(stale_execution());
    }
    sqlx::query(
        "UPDATE provider_turn_effects SET phase = CASE WHEN phase = 'dispatching' \
         THEN 'interrupt_ambiguous' ELSE 'recovery_required' END, updated_at = ? \
         WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
         AND phase IN ('prepared', 'claimed', 'dispatching', 'issued_waiting_quiescence')",
    )
    .bind(canonical_now())
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .execute(&mut *transaction)
    .await?;
    session.public.recovery_required = true;
    session.public.updated_at = Utc::now();
    save_session(&mut transaction, &session).await?;
    let state = session_state_event(&mut transaction, &session).await?;
    transaction.commit().await?;
    Ok(AgentTurnCommit {
        events: vec![state],
        next_assignments: Vec::new(),
    })
}

pub(crate) async fn terminalize_ordinary_execution(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    authority: ProviderTurnAuthority<'_>,
    terminal_phase: ProviderTurnExecutionPhase,
) -> Result<(), PersistenceError> {
    if !matches!(
        terminal_phase,
        ProviderTurnExecutionPhase::Completed
            | ProviderTurnExecutionPhase::Declined
            | ProviderTurnExecutionPhase::Failed
    ) || session.public.room_id != authority.room_id
        || session.public.session_id != authority.session_id
        || session.public.active_turn_id != authority.turn_id
        || session.turn_generation != authority.turn_generation
        || session.runtime_handle_id != authority.runtime_handle_id
        || session.runtime_owner_id != authority.runtime_owner_id
        || session.runtime_lease_token != authority.runtime_lease_token
    {
        return Err(stale_execution());
    }
    let participant = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(authority.room_id)
    .bind(&session.public.participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PersistenceError::ParticipantMissing)
    .and_then(|json| {
        serde_json::from_str::<agentsassemble_domain::Participant>(&json)
            .map_err(PersistenceError::from)
    })?;
    if participant.muted {
        return Err(stale_execution());
    }
    let updated = sqlx::query(
        "UPDATE provider_turn_executions SET phase = ?, provider_turn_id = ?, updated_at = ? \
         WHERE room_id = ? AND session_id = ? AND turn_generation = ? AND execution_id = ? \
         AND turn_id = ? AND phase IN ('start_dispatching', 'running') \
         AND start_dispatch_nonce = ? AND runtime_handle_id = ? AND runtime_owner_id = ? \
         AND runtime_lease_token = ? AND NOT EXISTS (SELECT 1 FROM provider_turn_effects effect \
           WHERE effect.room_id = provider_turn_executions.room_id \
           AND effect.session_id = provider_turn_executions.session_id \
           AND effect.turn_generation = provider_turn_executions.turn_generation \
           AND effect.phase != 'finalized')",
    )
    .bind(terminal_phase.as_str())
    .bind(authority.provider_turn_id)
    .bind(canonical_now())
    .bind(authority.room_id)
    .bind(authority.session_id)
    .bind(generation_i64(authority.turn_generation)?)
    .bind(authority.execution_id)
    .bind(authority.turn_id)
    .bind(authority.start_dispatch_nonce)
    .bind(authority.runtime_handle_id)
    .bind(authority.runtime_owner_id)
    .bind(authority.runtime_lease_token)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(stale_execution());
    }
    Ok(())
}

pub(crate) async fn load_execution_in(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    turn_generation: u64,
) -> Result<ProviderTurnExecution, PersistenceError> {
    let row = sqlx::query(
        "SELECT execution_id, participant_id, turn_id, phase, runtime_handle_id, \
         runtime_owner_id, runtime_lease_token, start_dispatch_nonce, provider_turn_id, \
         requeue_finalized FROM provider_turn_executions \
         WHERE room_id = ? AND session_id = ? AND turn_generation = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(generation_i64(turn_generation)?)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(stale_execution)?;
    Ok(ProviderTurnExecution {
        room_id: room_id.to_owned(),
        session_id: session_id.to_owned(),
        turn_generation,
        execution_id: row.get("execution_id"),
        participant_id: row.get("participant_id"),
        turn_id: row.get("turn_id"),
        phase: ProviderTurnExecutionPhase::parse(row.get::<String, _>("phase").as_str())?,
        runtime_handle_id: row.get("runtime_handle_id"),
        runtime_owner_id: row.get("runtime_owner_id"),
        runtime_lease_token: row.get("runtime_lease_token"),
        start_dispatch_nonce: row.get("start_dispatch_nonce"),
        provider_turn_id: row.get("provider_turn_id"),
        requeue_finalized: row.get::<i64, _>("requeue_finalized") != 0,
    })
}

fn generation_i64(generation: u64) -> Result<i64, PersistenceError> {
    i64::try_from(generation).map_err(|_| invalid_execution())
}

fn canonical_now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn invalid_execution() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stored_turn_execution_invalid",
        message: "Stored provider turn execution authority is invalid.".to_owned(),
    }
}

fn stale_execution() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_provider_turn",
        message: "Provider turn execution authority changed before this operation.".to_owned(),
    }
}
