use agentsassemble_domain::{
    AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession, Participant,
    ParticipantStatus, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_launch_events::commit_launch_result,
    agent_lifecycle_authority::{
        authorize_control, lifecycle_intent_is_empty, lifecycle_operation_id, payload_agent_id,
        require_intent, require_matching_operation, validate_runtime_started,
    },
    agent_lifecycle_reservations::{LifecycleReservation, finish_lifecycle_command},
    authority::active_room_for_principal,
    command_admission::existing_command,
    turn_authority::active_turn_authority,
    turn_queue::merge_room_inputs,
};

const START: &str = "agent.start";
const RESUME: &str = "agent.resume";

#[derive(Debug, Clone)]
pub struct AgentStartEffect {
    pub operation_id: String,
    pub session: DurableAgentSession,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentStopEffect {
    pub operation_id: String,
    pub session_id: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
}

#[derive(Debug, Clone)]
pub enum AgentStartPlan {
    Outcome(Box<CommandOutcome>),
    Start(Box<AgentStartEffect>),
}

#[derive(Debug, Clone)]
pub enum AgentStopPlan {
    Outcome(Box<CommandOutcome>),
    Stop(AgentStopEffect),
    Finalize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentRuntimeStarted {
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub provider_session_id: String,
    pub runtime_reused: bool,
    pub provider_session_reused: bool,
    pub provider_session_active: bool,
}

impl SqliteStore {
    /// Durably records a start intent before the provider supervisor is called.
    ///
    /// # Errors
    ///
    /// Returns authorization, idempotency, payload, state, or storage failures.
    pub async fn prepare_agent_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentStartPlan, PersistenceError> {
        self.prepare_agent_launch(principal, request_id, payload, START)
            .await
    }

    /// Durably records a resume intent before the provider supervisor is called.
    ///
    /// A stopped runtime resumes through the same provider launch effect as start,
    /// while retaining a distinct command identity for replay and conflict checks.
    ///
    /// # Errors
    ///
    /// Returns authorization, idempotency, payload, state, or storage failures.
    pub async fn prepare_agent_resume(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentStartPlan, PersistenceError> {
        self.prepare_agent_launch(principal, request_id, payload, RESUME)
            .await
    }

    async fn prepare_agent_launch(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        command_action: &'static str,
    ) -> Result<AgentStartPlan, PersistenceError> {
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let operation_id = lifecycle_operation_id(principal, request_id, command_action);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            command_action,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentStartPlan::Outcome(Box::new(outcome)));
        }
        let reservation = LifecycleReservation::new(
            principal,
            request_id,
            command_action,
            &payload_hash,
            &agent_id,
            &operation_id,
        );
        self.claim_lifecycle_command(&mut transaction, &reservation, payload, true)
            .await?;
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        require_valid_turn_authority(&session)?;
        let participant = load_participant(&mut transaction, &principal.room_id, &agent_id).await?;
        if participant.status == ParticipantStatus::Kicked {
            return Err(rejected(
                "participant_kicked",
                "This agent was removed from the room. Add it again before starting it.",
            ));
        }
        if session.runtime_profile_version != CURRENT_RUNTIME_PROFILE_VERSION {
            return Err(rejected(
                "runtime_profile_unsupported",
                "This Agent Session runtime profile is not supported by the current runtime.",
            ));
        }
        let incomplete = matching_start_intent(&mut session, &operation_id)?;
        let operation_id = if incomplete {
            save_session(&mut transaction, &session).await?;
            operation_id
        } else {
            "available".clone_into(&mut session.public.status);
            session.public.enabled = true;
            if !matches!(
                session.public.runtime_status.as_str(),
                "starting" | "idle" | "busy" | "paused"
            ) {
                "starting".clone_into(&mut session.public.runtime_status);
            }
            session.public.last_error.clear();
            session.public.last_error_code.clear();
            session.public.recovery_required = false;
            "start".clone_into(&mut session.lifecycle_intent_action);
            session.lifecycle_intent_id.clone_from(&operation_id);
            "prepared".clone_into(&mut session.lifecycle_intent_status);
            session.public.updated_at = Utc::now();
            save_session(&mut transaction, &session).await?;
            operation_id
        };
        transaction.commit().await?;
        Ok(AgentStartPlan::Start(Box::new(AgentStartEffect {
            operation_id,
            session,
        })))
    }

    /// Commits the observed provider start and the correlated command result.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn complete_agent_start(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        started: &AgentRuntimeStarted,
    ) -> Result<CommandOutcome, PersistenceError> {
        self.complete_agent_launch(principal, request_id, payload, operation_id, started, START)
            .await
    }

    /// Commits a resumed provider runtime and the correlated resume result.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn complete_agent_resume(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        started: &AgentRuntimeStarted,
    ) -> Result<CommandOutcome, PersistenceError> {
        self.complete_agent_launch(
            principal,
            request_id,
            payload,
            operation_id,
            started,
            RESUME,
        )
        .await
    }

    async fn complete_agent_launch(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        operation_id: &str,
        started: &AgentRuntimeStarted,
        command_action: &'static str,
    ) -> Result<CommandOutcome, PersistenceError> {
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, command_action);
        if operation_id != expected_operation_id {
            return Err(rejected(
                "stale_start_confirmation",
                "Provider start confirmation does not match its request.",
            ));
        }
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            command_action,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        validate_runtime_started(&session, started)?;
        require_intent(
            &session,
            START,
            &expected_operation_id,
            "effect_inflight",
            "stale_start_confirmation",
        )?;
        let reservation = LifecycleReservation::new(
            principal,
            request_id,
            command_action,
            &payload_hash,
            &agent_id,
            &expected_operation_id,
        );
        finish_lifecycle_command(&mut transaction, &reservation).await?;
        apply_runtime_started(&mut session, started);
        save_session(&mut transaction, &session).await?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &agent_id).await?;
        let joined = participant.status != ParticipantStatus::Joined;
        participant.status = ParticipantStatus::Joined;
        participant.updated_at = Utc::now();
        save_participant(&mut transaction, &participant).await?;
        let outcome = commit_launch_result(
            &mut transaction,
            principal,
            request_id,
            payload_hash,
            &session,
            &participant,
            joined,
            started.runtime_reused,
            command_action,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

fn matching_start_intent(
    session: &mut DurableAgentSession,
    operation_id: &str,
) -> Result<bool, PersistenceError> {
    if lifecycle_intent_is_empty(session) {
        return Ok(false);
    }
    require_matching_operation(session, "start", operation_id)?;
    match session.lifecycle_intent_status.as_str() {
        "prepared" => Ok(true),
        "effect_inflight" | "unconfirmed" => Err(unresolved_effect()),
        _ => Err(rejected(
            "invalid_state",
            "Stored provider start intent is invalid.",
        )),
    }
}

pub(crate) fn clear_intent(session: &mut DurableAgentSession) {
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
}

pub(crate) fn require_valid_turn_authority(
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    if active_turn_authority(session).is_ok() {
        Ok(())
    } else {
        Err(invalid_turn_queue())
    }
}

pub(crate) fn merged_turn_queue(
    session: &DurableAgentSession,
) -> Result<Vec<agentsassemble_domain::QueuedRoomInput>, PersistenceError> {
    merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| invalid_turn_queue())
}

pub(crate) fn invalid_turn_queue() -> PersistenceError {
    rejected(
        "stored_turn_authority_invalid",
        "Stored Agent Session turn authority is inconsistent or oversized.",
    )
}

pub(crate) fn unresolved_effect() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "runtime_effect_unconfirmed",
        message: "The original provider lifecycle effect remains unresolved. Wait for authoritative runtime observation before retrying it.".to_owned(),
    }
}

pub(crate) async fn load_session(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<DurableAgentSession, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? AND session_id = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(|| {
        rejected(
            "not_found",
            format!("Agent session {session_id} was not found."),
        )
    })?;
    Ok(serde_json::from_str(&encoded)?)
}

pub(crate) async fn save_session(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = ? AND session_id = ?",
    )
    .bind(serde_json::to_string(session)?)
    .bind(&session.public.room_id)
    .bind(&session.public.session_id)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(rejected("not_found", "Agent session was not found."));
    }
    Ok(())
}

pub(crate) async fn load_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PersistenceError::ParticipantMissing)?;
    Ok(serde_json::from_str(&encoded)?)
}

pub(crate) async fn save_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(participant)?)
    .bind(&participant.room_id)
    .bind(&participant.participant_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) fn apply_runtime_started(
    session: &mut DurableAgentSession,
    started: &AgentRuntimeStarted,
) {
    "attached".clone_into(&mut session.public.status);
    session.public.enabled = true;
    if !started.runtime_reused
        || !matches!(
            session.public.runtime_status.as_str(),
            "idle" | "busy" | "paused"
        )
    {
        "idle".clone_into(&mut session.public.runtime_status);
    }
    session.public.provider_session_active = started.provider_session_active;
    session.public.provider_session_reused = started.provider_session_reused;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session
        .runtime_handle_id
        .clone_from(&started.runtime_handle_id);
    session
        .runtime_owner_id
        .clone_from(&started.runtime_owner_id);
    session
        .runtime_lease_token
        .clone_from(&started.runtime_lease_token);
    if !started.provider_session_id.is_empty() {
        session
            .provider_session_id
            .clone_from(&started.provider_session_id);
    }
    clear_intent(session);
    session.public.updated_at = Utc::now();
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
#[path = "agent_lifecycle_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "agent_lifecycle_budget_tests.rs"]
mod budget_tests;

#[cfg(test)]
#[path = "agent_start_failure_tests.rs"]
mod start_failure_tests;

#[cfg(test)]
#[path = "agent_lifecycle_recovery_tests.rs"]
mod recovery_tests;

#[cfg(test)]
#[path = "agent_lifecycle_identity_recovery_tests.rs"]
mod identity_recovery_tests;

#[cfg(test)]
#[path = "agent_lifecycle_pre_effect_recovery_tests.rs"]
mod pre_effect_recovery_tests;

#[cfg(test)]
#[path = "agent_lifecycle_live_recovery_tests.rs"]
mod live_recovery_tests;

#[cfg(test)]
#[path = "agent_resume_tests.rs"]
mod resume_tests;

#[cfg(test)]
#[path = "agent_turn_recovery_tests.rs"]
mod turn_recovery_tests;
