use std::collections::{BTreeMap, HashSet};

use agentsassemble_domain::{
    AuthenticatedPrincipal, CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession, Participant,
    ParticipantStatus, RoomEvent, canonical_payload_hash, redact_persisted_diagnostic_text,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle_authority::{
        authorize_control, lifecycle_intent_is_empty, lifecycle_operation_id,
        matching_prepared_intent, payload_agent_id, require_intent, require_matching_operation,
        validate_runtime_started,
    },
    agent_lifecycle_events::{
        append_error_event, append_session_event, append_state_event, commit_already_stopped,
        commit_reused_start, store_result,
    },
    agent_lifecycle_reservations::{
        LifecycleReservation, claim_lifecycle_command, finish_lifecycle_command,
    },
    authority::active_room_for_principal,
    command_admission::existing_command,
};

const START: &str = "agent.start";
const STOP: &str = "agent.stop";
const PUBLIC_LIFECYCLE_ERROR_LIMIT: usize = 512;

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
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let operation_id = lifecycle_operation_id(principal, request_id, START);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            START,
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
            START,
            &payload_hash,
            &agent_id,
            &operation_id,
        );
        claim_lifecycle_command(&mut transaction, &reservation).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        let participant = load_participant(&mut transaction, &principal.room_id, &agent_id).await?;
        if participant.status == ParticipantStatus::Kicked {
            return Err(rejected(
                "participant_kicked",
                "This agent was removed from the room. Add it again before starting it.",
            ));
        }
        if session.runtime_profile_version != CURRENT_RUNTIME_PROFILE_VERSION {
            return Err(rejected(
                "profile_migration_required",
                "This Agent Session runtime profile must be saved again before it can start.",
            ));
        }
        let incomplete = matching_prepared_intent(&session, "start", &operation_id)?;
        if matches!(
            session.public.runtime_status.as_str(),
            "starting" | "idle" | "busy" | "paused"
        ) && !incomplete
        {
            finish_lifecycle_command(&mut transaction, &reservation).await?;
            let outcome = commit_reused_start(
                &mut transaction,
                principal,
                request_id,
                payload_hash,
                &session.public,
            )
            .await?;
            transaction.commit().await?;
            return Ok(AgentStartPlan::Outcome(Box::new(outcome)));
        }
        let operation_id = if incomplete {
            operation_id
        } else {
            "available".clone_into(&mut session.public.status);
            session.public.enabled = true;
            "starting".clone_into(&mut session.public.runtime_status);
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
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let expected_operation_id = lifecycle_operation_id(principal, request_id, START);
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
            START,
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
            "prepared",
            "stale_start_confirmation",
        )?;
        let reservation = LifecycleReservation::new(
            principal,
            request_id,
            START,
            &payload_hash,
            &agent_id,
            &expected_operation_id,
        );
        finish_lifecycle_command(&mut transaction, &reservation).await?;
        "attached".clone_into(&mut session.public.status);
        session.public.enabled = true;
        "idle".clone_into(&mut session.public.runtime_status);
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
        if !started.provider_session_id.is_empty() {
            session
                .provider_session_id
                .clone_from(&started.provider_session_id);
        }
        clear_intent(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &agent_id).await?;
        let joined = participant.status != ParticipantStatus::Joined;
        participant.status = ParticipantStatus::Joined;
        participant.updated_at = Utc::now();
        save_participant(&mut transaction, &participant).await?;
        let outcome = commit_start_result(
            &mut transaction,
            principal,
            request_id,
            payload_hash,
            &session,
            joined,
            started.runtime_reused,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Moves a failed start out of its prepared intent without hiding the error.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn fail_agent_start(
        &self,
        principal: &AuthenticatedPrincipal,
        agent_id: &str,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, agent_id).await?;
        require_intent(
            &session,
            START,
            operation_id,
            "prepared",
            "stale_start_confirmation",
        )?;
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "error".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.last_error =
            redact_persisted_diagnostic_text(message, PUBLIC_LIFECYCLE_ERROR_LIMIT);
        if session.public.last_error.is_empty() {
            "Provider launch failed.".clone_into(&mut session.public.last_error);
        }
        session.public.last_error_code = error_code.to_owned();
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        clear_intent(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let error = append_error_event(
            &mut transaction,
            principal,
            &session.public,
            error_code,
            &session.public.last_error,
        )
        .await?;
        let state = append_state_event(&mut transaction, principal, &session.public).await?;
        transaction.commit().await?;
        Ok(vec![error, state])
    }

    /// Durably records a stop intent before the provider supervisor is called.
    ///
    /// # Errors
    ///
    /// Returns authorization, idempotency, payload, state, or storage failures.
    pub async fn prepare_agent_stop(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AgentStopPlan, PersistenceError> {
        authorize_control(principal)?;
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let operation_id = lifecycle_operation_id(principal, request_id, STOP);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            STOP,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(AgentStopPlan::Outcome(Box::new(outcome)));
        }
        let reservation = LifecycleReservation::new(
            principal,
            request_id,
            STOP,
            &payload_hash,
            &agent_id,
            &operation_id,
        );
        claim_lifecycle_command(&mut transaction, &reservation).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        if matches!(
            session.public.runtime_status.as_str(),
            "stopped" | "available"
        ) && session.runtime_handle_id.is_empty()
            && session.runtime_owner_id.is_empty()
            && lifecycle_intent_is_empty(&session)
        {
            finish_lifecycle_command(&mut transaction, &reservation).await?;
            let outcome = commit_already_stopped(
                &mut transaction,
                principal,
                request_id,
                payload_hash,
                &session.public,
            )
            .await?;
            transaction.commit().await?;
            return Ok(AgentStopPlan::Outcome(Box::new(outcome)));
        }
        if !lifecycle_intent_is_empty(&session) {
            require_matching_operation(&session, "stop", &operation_id)?;
            if session.lifecycle_intent_status == "effect_applied" {
                transaction.commit().await?;
                return Ok(AgentStopPlan::Finalize);
            }
            if session.lifecycle_intent_status == "prepared" {
                let effect = stop_effect(&session)?;
                transaction.commit().await?;
                return Ok(AgentStopPlan::Stop(effect));
            }
            if session.lifecycle_intent_status == "unconfirmed" {
                let effect = stop_effect(&session)?;
                "prepared".clone_into(&mut session.lifecycle_intent_status);
                session.public.updated_at = Utc::now();
                save_session(&mut transaction, &session).await?;
                transaction.commit().await?;
                return Ok(AgentStopPlan::Stop(effect));
            }
            return Err(rejected(
                "invalid_state",
                "Stored provider stop intent is invalid.",
            ));
        }
        "stopping".clone_into(&mut session.public.runtime_status);
        session.public.enabled = false;
        "stop".clone_into(&mut session.lifecycle_intent_action);
        session.lifecycle_intent_id.clone_from(&operation_id);
        "prepared".clone_into(&mut session.lifecycle_intent_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let effect = stop_effect(&session)?;
        transaction.commit().await?;
        Ok(AgentStopPlan::Stop(effect))
    }

    /// Records confirmed shutdown before final state and command result writes.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn record_agent_stop_effect(
        &self,
        room_id: &str,
        session_id: &str,
        operation_id: &str,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let mut session = load_session(&mut transaction, room_id, session_id).await?;
        require_intent(
            &session,
            STOP,
            operation_id,
            "prepared",
            "stale_stop_confirmation",
        )?;
        "effect_applied".clone_into(&mut session.lifecycle_intent_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Makes an ambiguous stop visible and recoverable without claiming success.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn mark_agent_stop_unconfirmed(
        &self,
        principal: &AuthenticatedPrincipal,
        agent_id: &str,
        operation_id: &str,
        error_code: &'static str,
        message: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let mut session = load_session(&mut transaction, &principal.room_id, agent_id).await?;
        require_intent(
            &session,
            STOP,
            operation_id,
            "prepared",
            "stale_stop_confirmation",
        )?;
        session.pending_event_ids = dedupe(
            session
                .inflight_event_ids
                .iter()
                .chain(&session.pending_event_ids),
        );
        session.inflight_event_ids.clear();
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "disconnected".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.provider_session_reused = false;
        session.public.active_turn_id.clear();
        session.public.turn_phase.clear();
        session.public.last_error =
            redact_persisted_diagnostic_text(message, PUBLIC_LIFECYCLE_ERROR_LIMIT);
        if session.public.last_error.is_empty() {
            "Provider shutdown could not be confirmed.".clone_into(&mut session.public.last_error);
        }
        session.public.last_error_code = error_code.to_owned();
        session.public.recovery_required = true;
        "unconfirmed".clone_into(&mut session.lifecycle_intent_status);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, agent_id).await?;
        participant.status = ParticipantStatus::Detached;
        participant.updated_at = Utc::now();
        save_participant(&mut transaction, &participant).await?;
        let error = append_error_event(
            &mut transaction,
            principal,
            &session.public,
            error_code,
            &session.public.last_error,
        )
        .await?;
        let state = append_state_event(&mut transaction, principal, &session.public).await?;
        transaction.commit().await?;
        Ok(vec![error, state])
    }

    /// Finalizes a previously confirmed stop without repeating its external effect.
    ///
    /// # Errors
    ///
    /// Returns a stale-effect rejection or persistence failure.
    pub async fn finalize_agent_stop(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let agent_id = payload_agent_id(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            STOP,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        let operation_id = lifecycle_operation_id(principal, request_id, STOP);
        require_intent(
            &session,
            "stop",
            &operation_id,
            "effect_applied",
            "stale_stop_confirmation",
        )?;
        let reservation = LifecycleReservation::new(
            principal,
            request_id,
            STOP,
            &payload_hash,
            &agent_id,
            &operation_id,
        );
        finish_lifecycle_command(&mut transaction, &reservation).await?;
        session.pending_event_ids = dedupe(
            session
                .inflight_event_ids
                .iter()
                .chain(&session.pending_event_ids),
        );
        session.inflight_event_ids.clear();
        "detached".clone_into(&mut session.public.status);
        session.public.enabled = false;
        "stopped".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.active_turn_id.clear();
        session.public.turn_phase.clear();
        session.public.last_error.clear();
        session.public.last_error_code.clear();
        session.public.recovery_required = false;
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        clear_intent(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &agent_id).await?;
        participant.status = ParticipantStatus::Detached;
        participant.updated_at = Utc::now();
        save_participant(&mut transaction, &participant).await?;
        let detached = append_session_event(
            &mut transaction,
            principal,
            &session.public,
            "session_detached",
            BTreeMap::from([("reason".to_owned(), json!("operator stop"))]),
        )
        .await?;
        let state = append_state_event(&mut transaction, principal, &session.public).await?;
        let events = vec![detached, state];
        let result = json!({
            "agent_session": session.public,
            "process": {
                "stopped": true,
                "alive": false,
                "ownership": "server",
                "confirmed": true,
            },
            "revoked_sessions": 0,
            "events": events,
            "event": events.last(),
        });
        let outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            STOP,
            payload_hash,
            result,
            events,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn commit_start_result(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: String,
    session: &DurableAgentSession,
    joined: bool,
    runtime_reused: bool,
) -> Result<CommandOutcome, PersistenceError> {
    let mut events = Vec::with_capacity(3);
    if joined {
        events.push(
            append_session_event(
                transaction,
                principal,
                &session.public,
                "participant_joined",
                BTreeMap::new(),
            )
            .await?,
        );
    }
    events.push(
        append_session_event(
            transaction,
            principal,
            &session.public,
            "session_attached",
            BTreeMap::new(),
        )
        .await?,
    );
    events.push(append_state_event(transaction, principal, &session.public).await?);
    let result = json!({
        "agent_session": session.public,
        "runtime_reused": runtime_reused,
        "events": events,
        "event": events.last(),
    });
    store_result(
        transaction,
        principal,
        request_id,
        START,
        payload_hash,
        result,
        events,
    )
    .await
}

fn stop_effect(session: &DurableAgentSession) -> Result<AgentStopEffect, PersistenceError> {
    if session.runtime_handle_id.is_empty() || session.runtime_owner_id.is_empty() {
        return Err(rejected(
            "runtime_handle_unavailable",
            "Provider shutdown requires an exact handle owned by the current supervisor.",
        ));
    }
    Ok(AgentStopEffect {
        operation_id: session.lifecycle_intent_id.clone(),
        session_id: session.public.session_id.clone(),
        runtime_handle_id: session.runtime_handle_id.clone(),
        runtime_owner_id: session.runtime_owner_id.clone(),
    })
}

fn clear_intent(session: &mut DurableAgentSession) {
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
}

fn dedupe<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut seen = HashSet::new();
    values
        .filter(|value| !value.is_empty() && seen.insert((*value).clone()))
        .cloned()
        .collect()
}

async fn load_session(
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

async fn save_session(
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

async fn load_participant(
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

async fn save_participant(
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
#[path = "agent_lifecycle_recovery_tests.rs"]
mod recovery_tests;
