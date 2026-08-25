use agentsassemble_domain::{
    AuthenticatedPrincipal, DurableAgentSession, Participant, ParticipantStatus, Room, RoomStatus,
    canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError, SqliteStore, agent_lifecycle_authority::payload_agent_id,
    turn_authority::active_turn_authority, turn_queue::merge_room_inputs,
};

const ACTIVE_RUNTIME_STATES: [&str; 6] = [
    "starting",
    "idle",
    "busy",
    "paused",
    "recovering",
    "stopping",
];

#[derive(Debug, Clone)]
pub struct RuntimeReconciliationCandidate {
    pub session: DurableAgentSession,
    pub reservation: Option<RuntimeReconciliationReservation>,
    pub cas_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeReconciliationReservation {
    pub principal: AuthenticatedPrincipal,
    pub request_id: String,
    pub action: String,
    pub payload_hash: String,
    pub payload: Value,
    pub supervisor_generation: String,
    pub session_id: String,
    pub operation_id: String,
    pub phase: String,
    pub prepared_result_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeReconciliationObservation {
    Adopted {
        handle_id: String,
        previous_owner_id: String,
        new_owner_id: String,
        runtime_profile_key: String,
    },
    Gone,
    LeaseUncertain {
        handle_id: String,
        owner_id: String,
        reason_code: String,
    },
    Ambiguous {
        reason_code: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveRuntimeReconciliation {
    RetryOriginalEffect,
    StillUnresolved,
}

impl SqliteStore {
    /// Loads private runtime candidates without retaining a transaction across process I/O.
    ///
    /// # Errors
    ///
    /// Returns a persistence or stored-data failure.
    pub async fn load_runtime_reconciliation_candidates(
        &self,
    ) -> Result<Vec<RuntimeReconciliationCandidate>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let keys = sqlx::query(
            "SELECT sessions.room_id, sessions.session_id FROM agent_sessions AS sessions JOIN rooms ON rooms.room_id = sessions.room_id ORDER BY sessions.room_id, sessions.session_id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut candidates = Vec::new();
        for key in keys {
            let room_id = key.get::<String, _>("room_id");
            let session_id = key.get::<String, _>("session_id");
            if crate::provider_turn_execution::blocking_execution_exists(
                &mut transaction,
                &room_id,
                &session_id,
            )
            .await?
            {
                continue;
            }
            if let Some(candidate) = load_candidate(&mut transaction, &room_id, &session_id).await?
                && needs_reconciliation(&candidate.session)
            {
                candidates.push(candidate);
            }
        }
        transaction.commit().await?;
        Ok(candidates)
    }

    /// Reloads one private runtime candidate for a server-owned recovery watcher.
    ///
    /// # Errors
    ///
    /// Returns a stored-data or persistence failure.
    pub async fn load_runtime_reconciliation_candidate(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Option<RuntimeReconciliationCandidate>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        if crate::provider_turn_execution::blocking_execution_exists(
            &mut transaction,
            room_id,
            session_id,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(None);
        }
        let candidate = load_candidate(&mut transaction, room_id, session_id).await?;
        transaction.commit().await?;
        Ok(candidate.filter(|candidate| needs_reconciliation(&candidate.session)))
    }

    /// Applies an external observation only if its complete durable candidate is current.
    ///
    /// # Errors
    ///
    /// Returns `stale_reconciliation_candidate` when authority changed after the read.
    pub async fn apply_runtime_reconciliation(
        &self,
        candidate: &RuntimeReconciliationCandidate,
        observation: &RuntimeReconciliationObservation,
    ) -> Result<Vec<crate::AgentTurnAssignment>, PersistenceError> {
        crate::agent_reconciliation_recovery::apply_startup_reconciliation(
            self,
            candidate,
            observation,
        )
        .await
    }

    /// Applies exact runtime-gone authority during complete server shutdown without assigning
    /// new provider work to runtimes that are already being stopped.
    ///
    /// # Errors
    ///
    /// Returns `stale_reconciliation_candidate` when authority changed after the read.
    pub async fn apply_runtime_shutdown_reconciliation(
        &self,
        candidate: &RuntimeReconciliationCandidate,
        observation: &RuntimeReconciliationObservation,
    ) -> Result<(), PersistenceError> {
        crate::agent_reconciliation_recovery::apply_shutdown_reconciliation(
            self,
            candidate,
            observation,
        )
        .await
    }

    /// Applies one bounded observation for the exact command that still owns an unconfirmed
    /// lifecycle effect. Only proven absence or exact owned-runtime adoption can re-enable the
    /// original effect path.
    ///
    /// # Errors
    ///
    /// Returns an exact-CAS, observation, or stored-authority failure.
    pub async fn apply_live_runtime_reconciliation(
        &self,
        candidate: &RuntimeReconciliationCandidate,
        observation: &RuntimeReconciliationObservation,
    ) -> Result<LiveRuntimeReconciliation, PersistenceError> {
        crate::agent_reconciliation_recovery::apply_live_reconciliation(
            self,
            candidate,
            observation,
        )
        .await
    }

    /// Terminalizes an exact prepared lifecycle whose in-memory command owner is gone.
    ///
    /// # Errors
    ///
    /// Returns an exact-CAS, stored-authority, or persistence failure.
    pub async fn reject_abandoned_lifecycle_before_effect(
        &self,
        candidate: &RuntimeReconciliationCandidate,
    ) -> Result<(), PersistenceError> {
        crate::agent_reconciliation_recovery::reject_abandoned_lifecycle_before_effect(
            self, candidate,
        )
        .await
    }

    #[cfg(test)]
    /// Applies an ambiguous observation to every candidate for recovery tests.
    ///
    /// # Errors
    ///
    /// Returns a persistence or exact-CAS failure.
    pub async fn reconcile_agent_sessions_after_restart(&self) -> Result<usize, PersistenceError> {
        let candidates = self.load_runtime_reconciliation_candidates().await?;
        for candidate in &candidates {
            let assignments = self
                .apply_runtime_reconciliation(
                    candidate,
                    &RuntimeReconciliationObservation::Ambiguous {
                        reason_code: "test_owner_unavailable".to_owned(),
                    },
                )
                .await?;
            debug_assert!(assignments.is_empty());
        }
        Ok(candidates.len())
    }
}

pub(crate) async fn load_candidate(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<Option<RuntimeReconciliationCandidate>, PersistenceError> {
    let row = sqlx::query(
        "SELECT sessions.session_json, rooms.room_json FROM agent_sessions AS sessions JOIN rooms ON rooms.room_id = sessions.room_id WHERE sessions.room_id = ? AND sessions.session_id = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let room = serde_json::from_str::<Room>(&row.get::<String, _>("room_json"))?;
    if room.status == RoomStatus::Closed {
        return Ok(None);
    }
    let encoded_session = row.get::<String, _>("session_json");
    let session = serde_json::from_str::<DurableAgentSession>(&encoded_session)?;
    let reservation_rows = sqlx::query(
        "SELECT principal_id, request_id, action, payload_hash, principal_json, payload_json, supervisor_generation, operation_id, status, phase, prepared_result_json, failure_code, failure_message FROM lifecycle_command_reservations WHERE room_id = ? AND session_id = ? AND status = 'pending' ORDER BY principal_id, request_id",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_all(&mut **transaction)
    .await?;
    let reservations = reservation_rows
        .into_iter()
        .map(|row| {
            json!({
                "principal_id": row.get::<String, _>("principal_id"),
                "request_id": row.get::<String, _>("request_id"),
                "action": row.get::<String, _>("action"),
                "payload_hash": row.get::<String, _>("payload_hash"),
                "principal_json": row.get::<String, _>("principal_json"),
                "payload_json": row.get::<String, _>("payload_json"),
                "supervisor_generation": row.get::<String, _>("supervisor_generation"),
                "operation_id": row.get::<String, _>("operation_id"),
                "status": row.get::<String, _>("status"),
                "phase": row.get::<String, _>("phase"),
                "prepared_result_json": row.get::<String, _>("prepared_result_json"),
                "failure_code": row.get::<String, _>("failure_code"),
                "failure_message": row.get::<String, _>("failure_message"),
            })
        })
        .collect::<Vec<Value>>();
    let reservation = validate_candidate_authority(&session, &reservations)?;
    let cas_token = canonical_payload_hash(&json!({
        "session": serde_json::from_str::<Value>(&encoded_session)?,
        "reservations": reservations,
    }));
    Ok(Some(RuntimeReconciliationCandidate {
        session,
        reservation,
        cas_token,
    }))
}

fn validate_candidate_authority(
    session: &DurableAgentSession,
    reservations: &[Value],
) -> Result<Option<RuntimeReconciliationReservation>, PersistenceError> {
    if active_turn_authority(session).is_err() {
        return Err(invalid_stored_authority());
    }
    let runtime_identity_empty = [
        session.runtime_handle_id.as_str(),
        session.runtime_owner_id.as_str(),
        session.runtime_lease_token.as_str(),
    ]
    .map(str::is_empty);
    if !runtime_identity_empty.iter().all(|empty| *empty)
        && !runtime_identity_empty.iter().all(|empty| !*empty)
    {
        return Err(invalid_stored_authority());
    }
    let lifecycle_fields = [
        session.lifecycle_intent_action.as_str(),
        session.lifecycle_intent_id.as_str(),
        session.lifecycle_intent_status.as_str(),
    ];
    let pending_reservations = reservations
        .iter()
        .filter(|reservation| reservation["status"].as_str() == Some("pending"))
        .count();
    if lifecycle_fields.iter().all(|value| value.is_empty()) {
        return if pending_reservations == 0 {
            Ok(None)
        } else {
            Err(invalid_stored_authority())
        };
    }
    if lifecycle_fields.iter().any(|value| value.is_empty())
        || !matches!(session.lifecycle_intent_action.as_str(), "start" | "stop")
        || !matches!(
            (
                session.lifecycle_intent_action.as_str(),
                session.lifecycle_intent_status.as_str()
            ),
            ("start", "prepared" | "effect_inflight" | "unconfirmed")
                | (
                    "stop",
                    "prepared" | "effect_inflight" | "unconfirmed" | "effect_applied"
                )
        )
    {
        return Err(invalid_stored_authority());
    }
    let mut matching_reservations = reservations.iter().filter(|reservation| {
        reservation_matches_intent(reservation, &session.lifecycle_intent_action)
            && reservation["operation_id"].as_str() == Some(session.lifecycle_intent_id.as_str())
            && reservation["status"].as_str() == Some("pending")
    });
    let Some(matching) = matching_reservations.next() else {
        return Err(invalid_stored_authority());
    };
    if pending_reservations != 1 || matching_reservations.next().is_some() {
        return Err(invalid_stored_authority());
    }
    Ok(Some(decode_reservation(session, matching)?))
}

fn decode_reservation(
    session: &DurableAgentSession,
    matching: &Value,
) -> Result<RuntimeReconciliationReservation, PersistenceError> {
    let principal = serde_json::from_str::<AuthenticatedPrincipal>(
        matching["principal_json"]
            .as_str()
            .ok_or_else(invalid_stored_authority)?,
    )?;
    let payload = serde_json::from_str::<Value>(
        matching["payload_json"]
            .as_str()
            .ok_or_else(invalid_stored_authority)?,
    )?;
    let principal_id = matching["principal_id"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let request_id = matching["request_id"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let action = matching["action"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let payload_hash = matching["payload_hash"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let operation_id = matching["operation_id"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let supervisor_generation = matching["supervisor_generation"]
        .as_str()
        .filter(|generation| !generation.is_empty())
        .ok_or_else(invalid_stored_authority)?;
    let phase = matching["phase"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    let prepared_result_json = matching["prepared_result_json"]
        .as_str()
        .ok_or_else(invalid_stored_authority)?;
    if principal.room_id != session.public.room_id
        || principal.principal_id != principal_id
        || !principal.capabilities.agent_control
        || principal.participant_id.is_empty()
        || canonical_payload_hash(&payload) != payload_hash
        || payload_session_id(action, &payload, prepared_result_json, session)?
            != session.public.session_id
        || matching["failure_code"].as_str() != Some("")
        || matching["failure_message"].as_str() != Some("")
    {
        return Err(invalid_stored_authority());
    }
    Ok(RuntimeReconciliationReservation {
        principal,
        request_id: request_id.to_owned(),
        action: action.to_owned(),
        payload_hash: payload_hash.to_owned(),
        payload,
        supervisor_generation: supervisor_generation.to_owned(),
        session_id: session.public.session_id.clone(),
        operation_id: operation_id.to_owned(),
        phase: phase.to_owned(),
        prepared_result_json: prepared_result_json.to_owned(),
    })
}

fn payload_session_id<'a>(
    action: &str,
    payload: &'a Value,
    prepared_result_json: &str,
    session: &'a DurableAgentSession,
) -> Result<String, PersistenceError> {
    if action == "agent.create" {
        let prepared_result = serde_json::from_str::<Value>(prepared_result_json)
            .map_err(|_| invalid_stored_authority())?;
        let session_id = prepared_result
            .pointer("/agent_session/session_id")
            .and_then(Value::as_str)
            .ok_or_else(invalid_stored_authority)?;
        let participant_id = prepared_result
            .pointer("/participant/participant_id")
            .and_then(Value::as_str)
            .ok_or_else(invalid_stored_authority)?;
        return if prepared_result["status"].as_str() == Some("created")
            && session_id == session.public.session_id
            && participant_id == session.public.session_id
        {
            Ok(session_id.to_owned())
        } else {
            Err(invalid_stored_authority())
        };
    }
    payload_agent_id(payload).map_err(|_| invalid_stored_authority())
}

fn reservation_matches_intent(reservation: &Value, intent_action: &str) -> bool {
    matches!(
        (
            intent_action,
            reservation["action"].as_str(),
            reservation["phase"].as_str(),
        ),
        (
            "start",
            Some("agent.start" | "agent.resume"),
            Some("lifecycle_prepared")
        ) | ("start", Some("agent.create"), Some("creation_committed"))
            | ("stop", Some("agent.stop"), Some("lifecycle_prepared"))
    )
}

pub(crate) fn reconcile_observation(
    session: &mut DurableAgentSession,
    observation: &RuntimeReconciliationObservation,
) -> Result<bool, PersistenceError> {
    if confirmed_stop_needs_reconciliation(session) {
        reconcile_confirmed_stop(session)?;
        return Ok(true);
    }
    match observation {
        RuntimeReconciliationObservation::Adopted {
            handle_id,
            previous_owner_id,
            new_owner_id,
            runtime_profile_key,
        } => {
            validate_adoption(
                session,
                handle_id,
                previous_owner_id,
                new_owner_id,
                runtime_profile_key,
            )?;
            session.runtime_handle_id.clone_from(handle_id);
            session.runtime_owner_id.clone_from(new_owner_id);
            session.public.provider_session_active = false;
            if active_turn_authority(session).unwrap_or(false) {
                merge_inflight_events(session)?;
                "unavailable".clone_into(&mut session.public.status);
                session.public.enabled = false;
                "recovering".clone_into(&mut session.public.runtime_status);
                session.public.active_turn_id.clear();
                session.public.turn_phase.clear();
                "A provider turn was interrupted while its runtime was adopted after restart."
                    .clone_into(&mut session.public.last_error);
                "provider_turn_recovery_required".clone_into(&mut session.public.last_error_code);
                session.public.recovery_required = true;
                session.public.updated_at = Utc::now();
                return Ok(true);
            }
            session.public.updated_at = Utc::now();
            Ok(false)
        }
        RuntimeReconciliationObservation::Gone => reconcile_gone(session),
        RuntimeReconciliationObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        } => {
            validate_uncertain_lease(session, handle_id, owner_id, reason_code)?;
            retain_uncertain_runtime(session)?;
            Ok(true)
        }
        RuntimeReconciliationObservation::Ambiguous { reason_code } => {
            if reason_code.is_empty() {
                return Err(invalid_observation());
            }
            retain_uncertain_runtime(session)?;
            Ok(true)
        }
    }
}

pub(crate) fn reconcile_gone(session: &mut DurableAgentSession) -> Result<bool, PersistenceError> {
    if session.lifecycle_intent_action == "stop"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "effect_inflight" | "unconfirmed"
        )
    {
        "effect_applied".clone_into(&mut session.lifecycle_intent_status);
        reconcile_confirmed_stop(session)?;
        return Ok(true);
    }
    if session.lifecycle_intent_action == "start"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "effect_inflight" | "unconfirmed"
        )
    {
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        session.runtime_lease_token.clear();
        "prepared".clone_into(&mut session.lifecycle_intent_status);
        "starting".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.updated_at = Utc::now();
        return Ok(false);
    }
    if ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str()) {
        disconnect_after_restart(session)?;
        return Ok(true);
    }
    Ok(invalidate_previous_runtime_owner(session))
}

pub(crate) fn validate_uncertain_lease(
    session: &DurableAgentSession,
    handle_id: &str,
    owner_id: &str,
    reason_code: &str,
) -> Result<(), PersistenceError> {
    if handle_id.is_empty()
        || owner_id.is_empty()
        || reason_code.is_empty()
        || handle_id != session.runtime_handle_id
        || owner_id != session.runtime_owner_id
    {
        return Err(invalid_observation());
    }
    Ok(())
}

pub(crate) fn validate_adoption(
    session: &DurableAgentSession,
    handle_id: &str,
    previous_owner_id: &str,
    new_owner_id: &str,
    runtime_profile_key: &str,
) -> Result<(), PersistenceError> {
    if handle_id.is_empty()
        || previous_owner_id.is_empty()
        || new_owner_id.is_empty()
        || handle_id != session.runtime_handle_id
        || previous_owner_id != session.runtime_owner_id
        || runtime_profile_key != session.runtime_profile_key
    {
        return Err(invalid_observation());
    }
    Ok(())
}

fn needs_reconciliation(session: &DurableAgentSession) -> bool {
    if session.lifecycle_intent_action == "stop"
        && session.lifecycle_intent_status == "effect_applied"
    {
        return confirmed_stop_needs_reconciliation(session);
    }
    ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str())
        || !session.runtime_handle_id.is_empty()
        || !session.runtime_owner_id.is_empty()
        || !session.runtime_lease_token.is_empty()
        || !session.lifecycle_intent_action.is_empty()
        || !session.lifecycle_intent_id.is_empty()
        || !session.lifecycle_intent_status.is_empty()
}

fn confirmed_stop_needs_reconciliation(session: &DurableAgentSession) -> bool {
    session.lifecycle_intent_action == "stop"
        && session.lifecycle_intent_status == "effect_applied"
        && (!session.runtime_handle_id.is_empty()
            || !session.runtime_owner_id.is_empty()
            || !session.runtime_lease_token.is_empty()
            || session.public.provider_session_active
            || session.public.provider_session_reused
            || !session.public.active_turn_id.is_empty()
            || !session.public.turn_phase.is_empty()
            || !session.inflight_inputs.is_empty()
            || session.public.status != "unavailable")
}

fn reconcile_confirmed_stop(session: &mut DurableAgentSession) -> Result<(), PersistenceError> {
    merge_inflight_events(session)?;
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "stopping".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.runtime_lease_token.clear();
    session.public.updated_at = Utc::now();
    Ok(())
}

fn retain_uncertain_runtime(session: &mut DurableAgentSession) -> Result<(), PersistenceError> {
    merge_inflight_events(session)?;
    if matches!(session.lifecycle_intent_action.as_str(), "start" | "stop")
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "effect_inflight"
        )
    {
        "unconfirmed".clone_into(&mut session.lifecycle_intent_status);
    }
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "disconnected".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.public.recovery_required = true;
    "Provider runtime authority could not be confirmed.".clone_into(&mut session.public.last_error);
    "runtime_authority_uncertain".clone_into(&mut session.public.last_error_code);
    session.public.updated_at = Utc::now();
    Ok(())
}

fn disconnect_after_restart(session: &mut DurableAgentSession) -> Result<(), PersistenceError> {
    disconnect_common(session)?;
    "Server restarted without a current owned provider handle."
        .clone_into(&mut session.public.last_error);
    "server_restarted".clone_into(&mut session.public.last_error_code);
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
    Ok(())
}

fn disconnect_common(session: &mut DurableAgentSession) -> Result<(), PersistenceError> {
    merge_inflight_events(session)?;
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "disconnected".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.public.recovery_required = true;
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.runtime_lease_token.clear();
    session.public.updated_at = Utc::now();
    Ok(())
}

fn invalidate_previous_runtime_owner(session: &mut DurableAgentSession) -> bool {
    if session.runtime_handle_id.is_empty()
        && session.runtime_owner_id.is_empty()
        && session.runtime_lease_token.is_empty()
    {
        return false;
    }
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.runtime_lease_token.clear();
    session.public.updated_at = Utc::now();
    true
}

fn merge_inflight_events(session: &mut DurableAgentSession) -> Result<(), PersistenceError> {
    session.pending_inputs = merge_room_inputs(
        session
            .inflight_inputs
            .iter()
            .chain(&session.pending_inputs),
    )
    .map_err(|_| invalid_stored_authority())?;
    session.inflight_inputs.clear();
    session.active_source_event_id.clear();
    session.input_up_to_event_id.clear();
    session.input_up_to_seq = 0;
    Ok(())
}

pub(crate) async fn save_reconciled_session(
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
        return Err(stale_candidate());
    }
    Ok(())
}

pub(crate) async fn detach_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    let Some(encoded) = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    else {
        return Ok(());
    };
    let mut participant = serde_json::from_str::<Participant>(&encoded)?;
    participant.status = ParticipantStatus::Detached;
    participant.updated_at = Utc::now();
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(&participant)?)
    .bind(room_id)
    .bind(participant_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) fn stale_candidate() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_reconciliation_candidate",
        message: "Provider runtime reconciliation candidate changed during observation.".to_owned(),
    }
}

pub(crate) fn invalid_observation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_runtime_observation",
        message: "Provider runtime observation does not match durable authority.".to_owned(),
    }
}

pub(crate) fn invalid_stored_authority() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_stored_runtime_authority",
        message: "Stored provider runtime authority is incomplete or inconsistent.".to_owned(),
    }
}
