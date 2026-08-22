use agentsassemble_domain::{
    DurableAgentSession, Participant, ParticipantStatus, Room, RoomStatus,
};
use chrono::Utc;
use sqlx::Row;

use crate::{PersistenceError, SqliteStore, agent_lifecycle_reservations::mark_stop_owner_lost};

const ACTIVE_RUNTIME_STATES: [&str; 6] = [
    "starting",
    "idle",
    "busy",
    "paused",
    "recovering",
    "stopping",
];

impl SqliteStore {
    /// Disconnects durable sessions whose process ownership cannot survive restart.
    ///
    /// This runs before HTTP or WebSocket admission. Provider conversation IDs stay
    /// durable so a later explicit start can resume them. Unresolved stops become a
    /// terminal owner-lost reservation before their session intent is released.
    ///
    /// # Errors
    ///
    /// Returns a persistence or stored-data failure and leaves the transaction rolled back.
    pub async fn reconcile_agent_sessions_after_restart(&self) -> Result<usize, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let rows = sqlx::query(
            "SELECT sessions.session_json, rooms.room_json FROM agent_sessions AS sessions JOIN rooms ON rooms.room_id = sessions.room_id ORDER BY sessions.room_id, sessions.session_id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut reconciled = 0_usize;
        for row in rows {
            let room = serde_json::from_str::<Room>(&row.get::<String, _>("room_json"))?;
            if room.status == RoomStatus::Closed {
                continue;
            }
            let mut session =
                serde_json::from_str::<DurableAgentSession>(&row.get::<String, _>("session_json"))?;
            if unresolved_stop_lost_its_owner(&session) {
                mark_stop_owner_lost(
                    &mut transaction,
                    &session.public.room_id,
                    &session.public.session_id,
                    &session.lifecycle_intent_id,
                )
                .await?;
                disconnect_after_owner_loss(&mut session);
                save_reconciled_session(&mut transaction, &session).await?;
                detach_participant(
                    &mut transaction,
                    &session.public.room_id,
                    &session.public.participant_id,
                )
                .await?;
                reconciled += 1;
                continue;
            }
            if confirmed_stop_needs_reconciliation(&session) {
                reconcile_confirmed_stop(&mut session);
                save_reconciled_session(&mut transaction, &session).await?;
                detach_participant(
                    &mut transaction,
                    &session.public.room_id,
                    &session.public.participant_id,
                )
                .await?;
                reconciled += 1;
                continue;
            }
            if session.lifecycle_intent_action == "stop"
                && session.lifecycle_intent_status == "effect_applied"
            {
                continue;
            }
            if !ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str())
                && invalidate_previous_runtime_owner(&mut session)
            {
                save_reconciled_session(&mut transaction, &session).await?;
                reconciled += 1;
                continue;
            }
            if !ACTIVE_RUNTIME_STATES.contains(&session.public.runtime_status.as_str()) {
                continue;
            }
            disconnect_after_restart(&mut session);
            save_reconciled_session(&mut transaction, &session).await?;
            detach_participant(
                &mut transaction,
                &session.public.room_id,
                &session.public.participant_id,
            )
            .await?;
            reconciled += 1;
        }
        transaction.commit().await?;
        Ok(reconciled)
    }
}

fn unresolved_stop_lost_its_owner(session: &DurableAgentSession) -> bool {
    session.lifecycle_intent_action == "stop"
        && matches!(
            session.lifecycle_intent_status.as_str(),
            "prepared" | "unconfirmed"
        )
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
    merge_inflight_events(session);
    "unavailable".clone_into(&mut session.public.status);
    session.public.enabled = false;
    "disconnected".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.public.recovery_required = true;
    "Provider runtime ownership was lost during restart."
        .clone_into(&mut session.public.last_error);
    "runtime_owner_lost".clone_into(&mut session.public.last_error_code);
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
    session.public.updated_at = Utc::now();
}

fn disconnect_after_restart(session: &mut DurableAgentSession) {
    merge_inflight_events(session);
    "unavailable".clone_into(&mut session.public.status);
    "disconnected".clone_into(&mut session.public.runtime_status);
    session.public.provider_session_active = false;
    session.public.provider_session_reused = false;
    session.public.active_turn_id.clear();
    session.public.turn_phase.clear();
    session.public.recovery_required = true;
    "Server restarted without a current owned provider handle."
        .clone_into(&mut session.public.last_error);
    "server_restarted".clone_into(&mut session.public.last_error_code);
    session.runtime_handle_id.clear();
    session.runtime_owner_id.clear();
    session.lifecycle_intent_action.clear();
    session.lifecycle_intent_id.clear();
    session.lifecycle_intent_status.clear();
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
    let mut pending = Vec::new();
    for event_id in session
        .inflight_event_ids
        .iter()
        .chain(&session.pending_event_ids)
    {
        if !event_id.is_empty() && !pending.contains(event_id) {
            pending.push(event_id.clone());
        }
    }
    session.pending_event_ids = pending;
    session.inflight_event_ids.clear();
}

async fn save_reconciled_session(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    sqlx::query("UPDATE agent_sessions SET session_json = ? WHERE room_id = ? AND session_id = ?")
        .bind(serde_json::to_string(session)?)
        .bind(&session.public.room_id)
        .bind(&session.public.session_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn detach_participant(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
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
