use std::collections::BTreeMap;

use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, ParticipantStatus, RoomEvent,
    canonical_payload_hash, redact_persisted_diagnostic_text,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::{
    PersistenceError, RoomCommandMutation, SqliteStore,
    agent_lifecycle::{
        AgentStopPlan, invalid_turn_queue, load_participant, load_session, merged_turn_queue,
        require_valid_turn_authority, save_participant, save_session, unresolved_effect,
    },
    agent_lifecycle_authority::{
        agent_stop_requires_cleanup, authorize_control, lifecycle_intent_is_empty,
        lifecycle_operation_id, payload_agent_id, require_intent, require_matching_operation,
    },
    agent_lifecycle_effect_authority::stop_effect,
    agent_lifecycle_events::{
        append_error_event, append_session_event, append_state_event, commit_already_stopped,
        store_result,
    },
    agent_lifecycle_reservations::{LifecycleReservation, finish_lifecycle_command},
    authority::active_room_for_principal,
    command_admission::existing_command,
    room_turns::{assign_pending_in, support::load_active_room},
    turn_authority::active_turn_authority,
};

const STOP: &str = "agent.stop";
const PUBLIC_LIFECYCLE_ERROR_LIMIT: usize = 512;

impl SqliteStore {
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
        let mut session = load_session(&mut transaction, &principal.room_id, &agent_id).await?;
        require_valid_turn_authority(&session)?;
        let cleanup_required = agent_stop_requires_cleanup(&session);
        self.claim_lifecycle_command(&mut transaction, &reservation, payload, !cleanup_required)
            .await?;
        if !cleanup_required {
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
            if matches!(
                session.lifecycle_intent_status.as_str(),
                "effect_inflight" | "unconfirmed"
            ) {
                return Err(unresolved_effect());
            }
            return Err(rejected(
                "invalid_state",
                "Stored provider stop intent is invalid.",
            ));
        }
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
        let expected_status = if session.lifecycle_intent_status == "effect_applied" {
            "effect_applied"
        } else {
            "effect_inflight"
        };
        require_intent(
            &session,
            STOP,
            operation_id,
            expected_status,
            "stale_stop_confirmation",
        )?;
        crate::provider_turn_stop::terminalize_confirmed_stop_turn(&mut transaction, &session)
            .await?;
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
            "effect_inflight",
            "stale_stop_confirmation",
        )?;
        let active_turn = active_turn_authority(&session).map_err(|_| invalid_turn_queue())?;
        if !active_turn {
            session.pending_inputs = merged_turn_queue(&session)?;
            session.inflight_inputs.clear();
            "disconnected".clone_into(&mut session.public.runtime_status);
            session.public.provider_session_active = false;
            session.public.provider_session_reused = false;
            session.public.active_turn_id.clear();
            session.public.turn_phase.clear();
            session.active_source_event_id.clear();
            session.input_up_to_event_id.clear();
            session.input_up_to_seq = 0;
        }
        "unavailable".clone_into(&mut session.public.status);
        session.public.enabled = false;
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
    ) -> Result<RoomCommandMutation, PersistenceError> {
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
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        let (room, settings) = load_active_room(&mut transaction, &principal.room_id).await?;
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
        let events =
            detach_confirmed_session(&mut transaction, principal, &agent_id, &mut session).await?;
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
        let mut outcome = store_result(
            &mut transaction,
            principal,
            request_id,
            STOP,
            payload_hash,
            result,
            events,
        )
        .await?;
        let scheduled = assign_pending_in(&mut transaction, &room, &settings).await?;
        outcome.events.extend(scheduled.events);
        transaction.commit().await?;
        Ok(RoomCommandMutation {
            outcome,
            assignments: scheduled.next_assignments,
        })
    }
}

async fn detach_confirmed_session(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    agent_id: &str,
    session: &mut DurableAgentSession,
) -> Result<Vec<RoomEvent>, PersistenceError> {
    session.pending_inputs = merged_turn_queue(session)?;
    session.inflight_inputs.clear();
    "detached".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "stopped".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.active_source_event_id.clear();
    session.input_up_to_event_id.clear();
    session.input_up_to_seq = 0;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.runtime_lease_token.clear();
    crate::agent_lifecycle::clear_intent(session);
    session.public.updated_at = Utc::now();
    save_session(transaction, session).await?;
    let mut participant = load_participant(transaction, &principal.room_id, agent_id).await?;
    participant.status = ParticipantStatus::Detached;
    participant.updated_at = Utc::now();
    save_participant(transaction, &participant).await?;
    let detached = append_session_event(
        transaction,
        principal,
        &session.public,
        "session_detached",
        BTreeMap::from([("reason".to_owned(), json!("operator stop"))]),
    )
    .await?;
    let state = append_state_event(transaction, principal, &session.public).await?;
    Ok(vec![detached, state])
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
