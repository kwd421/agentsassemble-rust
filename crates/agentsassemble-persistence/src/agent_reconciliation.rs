use agentsassemble_domain::{
    DurableAgentSession, Participant, ParticipantStatus, Room, RoomStatus, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError, SqliteStore, agent_lifecycle_reservations::mark_lifecycle_owner_lost,
    turn_queue::bounded_event_ids,
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
    pub cas_token: String,
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
            if let Some(candidate) = load_candidate(&mut transaction, &room_id, &session_id).await?
                && needs_reconciliation(&candidate.session)
            {
                candidates.push(candidate);
            }
        }
        transaction.commit().await?;
        Ok(candidates)
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
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let Some(current) = load_candidate(
            &mut transaction,
            &candidate.session.public.room_id,
            &candidate.session.public.session_id,
        )
        .await?
        else {
            return Err(stale_candidate());
        };
        if current.cas_token != candidate.cas_token || current.session != candidate.session {
            return Err(stale_candidate());
        }
        let mut session = current.session;
        let detach = reconcile_observation(&mut transaction, &mut session, observation).await?;
        save_reconciled_session(&mut transaction, &session).await?;
        if detach {
            detach_participant(
                &mut transaction,
                &session.public.room_id,
                &session.public.participant_id,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    #[cfg(test)]
    /// Applies an ambiguous observation to every candidate for legacy recovery tests.
    ///
    /// # Errors
    ///
    /// Returns a persistence or exact-CAS failure.
    pub async fn reconcile_agent_sessions_after_restart(&self) -> Result<usize, PersistenceError> {
        let candidates = self.load_runtime_reconciliation_candidates().await?;
        for candidate in &candidates {
            self.apply_runtime_reconciliation(
                candidate,
                &RuntimeReconciliationObservation::Ambiguous {
                    reason_code: "test_owner_unavailable".to_owned(),
                },
            )
            .await?;
        }
        Ok(candidates.len())
    }
}

async fn load_candidate(
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
        "SELECT principal_id, request_id, action, payload_hash, operation_id, status FROM lifecycle_command_reservations WHERE room_id = ? AND session_id = ? ORDER BY principal_id, request_id",
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
                "operation_id": row.get::<String, _>("operation_id"),
                "status": row.get::<String, _>("status"),
            })
        })
        .collect::<Vec<Value>>();
    validate_candidate_authority(&session, &reservations)?;
    let cas_token = canonical_payload_hash(&json!({
        "session": serde_json::from_str::<Value>(&encoded_session)?,
        "reservations": reservations,
    }));
    Ok(Some(RuntimeReconciliationCandidate { session, cas_token }))
}

fn validate_candidate_authority(
    session: &DurableAgentSession,
    reservations: &[Value],
) -> Result<(), PersistenceError> {
    if session.runtime_handle_id.is_empty() != session.runtime_owner_id.is_empty() {
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
            Ok(())
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
            ("start", "prepared" | "unconfirmed")
                | ("stop", "prepared" | "unconfirmed" | "effect_applied")
        )
    {
        return Err(invalid_stored_authority());
    }
    let expected_action = format!("agent.{}", session.lifecycle_intent_action);
    let matching_reservations = reservations
        .iter()
        .filter(|reservation| {
            reservation["action"].as_str() == Some(expected_action.as_str())
                && reservation["operation_id"].as_str()
                    == Some(session.lifecycle_intent_id.as_str())
                && reservation["status"].as_str() == Some("pending")
        })
        .count();
    if pending_reservations != 1 || matching_reservations != 1 {
        return Err(invalid_stored_authority());
    }
    Ok(())
}

async fn reconcile_observation(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
    observation: &RuntimeReconciliationObservation,
) -> Result<bool, PersistenceError> {
    if confirmed_stop_needs_reconciliation(session) {
        reconcile_confirmed_stop(session);
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
            session.public.updated_at = Utc::now();
            Ok(false)
        }
        RuntimeReconciliationObservation::Gone => Ok(reconcile_gone(session)),
        RuntimeReconciliationObservation::LeaseUncertain {
            handle_id,
            owner_id,
            reason_code,
        } => {
            validate_uncertain_lease(session, handle_id, owner_id, reason_code)?;
            retain_uncertain_runtime(session);
            Ok(true)
        }
        RuntimeReconciliationObservation::Ambiguous { reason_code } => {
            if reason_code.is_empty() {
                return Err(invalid_observation());
            }
            reconcile_ambiguous(transaction, session).await
        }
    }
}

fn reconcile_gone(session: &mut DurableAgentSession) -> bool {
    if session.lifecycle_intent_action == "stop"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "unconfirmed"
        )
    {
        "effect_applied".clone_into(&mut session.lifecycle_intent_status);
        reconcile_confirmed_stop(session);
        return true;
    }
    if session.lifecycle_intent_action == "start"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "unconfirmed"
        )
    {
        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        "prepared".clone_into(&mut session.lifecycle_intent_status);
        "starting".clone_into(&mut session.public.runtime_status);
        session.public.provider_session_active = false;
        session.public.updated_at = Utc::now();
        return false;
    }
    if ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str()) {
        disconnect_after_restart(session);
        return true;
    }
    invalidate_previous_runtime_owner(session);
    false
}

async fn reconcile_ambiguous(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
) -> Result<bool, PersistenceError> {
    if session.lifecycle_intent_action == "start"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "unconfirmed"
        )
    {
        retain_uncertain_runtime(session);
        return Ok(true);
    }
    if session.lifecycle_intent_action == "stop"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "unconfirmed"
        )
    {
        let action = format!("agent.{}", session.lifecycle_intent_action);
        mark_lifecycle_owner_lost(
            transaction,
            &session.public.room_id,
            &session.public.session_id,
            &action,
            &session.lifecycle_intent_id,
        )
        .await?;
        disconnect_after_owner_loss(session);
        return Ok(true);
    }
    if ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str()) {
        disconnect_after_restart(session);
        return Ok(true);
    }
    invalidate_previous_runtime_owner(session);
    Ok(false)
}

fn validate_uncertain_lease(
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

fn validate_adoption(
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
        || !session.lifecycle_intent_action.is_empty()
        || !session.lifecycle_intent_id.is_empty()
        || !session.lifecycle_intent_status.is_empty()
}

fn confirmed_stop_needs_reconciliation(session: &DurableAgentSession) -> bool {
    session.lifecycle_intent_action == "stop"
        && session.lifecycle_intent_status == "effect_applied"
        && (!session.runtime_handle_id.is_empty()
            || !session.runtime_owner_id.is_empty()
            || session.public.provider_session_active
            || session.public.provider_session_reused
            || !session.public.active_turn_id.is_empty()
            || !session.public.turn_phase.is_empty()
            || !session.inflight_event_ids.is_empty()
            || session.public.status != "unavailable")
}

fn reconcile_confirmed_stop(session: &mut DurableAgentSession) {
    merge_inflight_events(session);
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "stopping".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.public.updated_at = Utc::now();
}

fn disconnect_after_owner_loss(session: &mut DurableAgentSession) {
    disconnect_common(session);
    "Provider runtime ownership was lost during restart."
        .clone_into(&mut session.public.last_error);
    "runtime_owner_lost".clone_into(&mut session.public.last_error_code);
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
}

fn retain_uncertain_runtime(session: &mut DurableAgentSession) {
    merge_inflight_events(session);
    if session.lifecycle_intent_action == "start" && session.lifecycle_intent_status == "prepared" {
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
}

fn disconnect_after_restart(session: &mut DurableAgentSession) {
    disconnect_common(session);
    "Server restarted without a current owned provider handle."
        .clone_into(&mut session.public.last_error);
    "server_restarted".clone_into(&mut session.public.last_error_code);
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
}

fn disconnect_common(session: &mut DurableAgentSession) {
    merge_inflight_events(session);
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
    session.public.updated_at = Utc::now();
}

fn invalidate_previous_runtime_owner(session: &mut DurableAgentSession) -> bool {
    if session.runtime_handle_id.is_empty() && session.runtime_owner_id.is_empty() {
        return false;
    }
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.public.updated_at = Utc::now();
    true
}

fn merge_inflight_events(session: &mut DurableAgentSession) {
    session.pending_event_ids = bounded_event_ids(
        session
            .inflight_event_ids
            .iter()
            .chain(&session.pending_event_ids),
    );
    session.inflight_event_ids.clear();
    session.active_source_event_id.clear();
    session.input_up_to_event_id.clear();
    session.input_up_to_seq = 0;
}

async fn save_reconciled_session(
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

async fn detach_participant(
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

fn stale_candidate() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_reconciliation_candidate",
        message: "Provider runtime reconciliation candidate changed during observation.".to_owned(),
    }
}

fn invalid_observation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_runtime_observation",
        message: "Provider runtime observation does not match durable authority.".to_owned(),
    }
}

fn invalid_stored_authority() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_stored_runtime_authority",
        message: "Stored provider runtime authority is incomplete or inconsistent.".to_owned(),
    }
}
