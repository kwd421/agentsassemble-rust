//! Ordered floor custody is independent of the exact provider execution's recovery custody.
use agentsassemble_domain::{DurableAgentSession, RoomInputDeliveryKind};
use sqlx::{Sqlite, Transaction};

use crate::{
    PersistenceError,
    room_turns::{
        scheduler::route_released_floor,
        support::{load_event, load_room_with_settings},
    },
};

pub(crate) async fn is_quarantined(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    Ok((session.public.recovery_required
        || (session.lifecycle_intent_action == agentsassemble_domain::AgentLifecycleAction::Stop
            && session.lifecycle_intent_status
                == agentsassemble_domain::AgentLifecycleIntentStatus::EffectApplied))
        && has_receipt(tx, session).await?)
}

/// NULL means no release attempt; an array is the receipt for this generation.
/// A receipt only contains inputs actually queued to another speaker, never direct targets.
pub(crate) async fn release_inputs(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    if has_receipt(tx, session).await? {
        return Ok(());
    }
    let (room, settings) = load_room_with_settings(tx, &session.public.room_id).await?;
    let mut released = Vec::new();
    for input in &session.inflight_inputs {
        if room.status != agentsassemble_domain::RoomStatus::Active
            || input.delivery_kind != RoomInputDeliveryKind::OrderedObservation
        {
            continue;
        }
        let event = load_event(tx, &session.public.room_id, &input.event_id)
            .await?
            .ok_or_else(crate::agent_lifecycle::invalid_turn_queue)?;
        if route_released_floor(tx, &settings, &event).await? {
            released.push(input.event_id.clone());
        }
    }
    sqlx::query("UPDATE provider_turn_executions SET released_input_ids = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ? AND released_input_ids IS NULL")
        .bind(serde_json::to_string(&released)?).bind(&session.public.room_id)
        .bind(&session.public.session_id).bind(i64::try_from(session.turn_generation).map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?)
        .execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn released_inputs(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<Vec<String>, PersistenceError> {
    if session.turn_generation == 0 {
        return Ok(Vec::new());
    }
    let receipt: Option<String> = sqlx::query_scalar(
        "SELECT released_input_ids FROM provider_turn_executions WHERE room_id = ? AND session_id = ? AND turn_generation = ?",
    ).bind(&session.public.room_id).bind(&session.public.session_id)
        .bind(i64::try_from(session.turn_generation).map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?)
        .fetch_one(&mut **tx).await?;
    receipt.map_or_else(|| Ok(Vec::new()), |json| Ok(serde_json::from_str(&json)?))
}

pub(crate) async fn has_released_inputs(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    Ok(!released_inputs(tx, session).await?.is_empty())
}

async fn has_receipt(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    sqlx::query_scalar::<_, bool>(
        "SELECT released_input_ids IS NOT NULL FROM provider_turn_executions WHERE room_id = ? AND session_id = ? AND turn_generation = ?",
    ).bind(&session.public.room_id).bind(&session.public.session_id)
        .bind(i64::try_from(session.turn_generation).map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?)
        .fetch_one(&mut **tx).await.map_err(PersistenceError::from)
}

/// Call only after the caller proves the exact transition's execution/effect authority.
/// Execution custody, public recovery, receipt and queued wake commit as one unit.
pub(crate) async fn quarantine(
    tx: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
) -> Result<crate::AgentTurnCommit, PersistenceError> {
    let execution = crate::provider_turn_execution::load_execution_in(
        tx,
        &session.public.room_id,
        &session.public.session_id,
        session.turn_generation,
    )
    .await?;
    if !crate::turn_authority::active_turn_authority(session)
        .map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?
        || !execution.phase.is_blocking()
        || execution.turn_id != session.public.active_turn_id
        || execution.participant_id != session.public.participant_id
        || execution.runtime_handle_id != session.runtime_handle_id
        || execution.runtime_owner_id != session.runtime_owner_id
        || execution.runtime_lease_token != session.runtime_lease_token
    {
        return Err(crate::agent_lifecycle::invalid_turn_queue());
    }
    let changed = !session.public.recovery_required || !has_receipt(tx, session).await?;
    sqlx::query("UPDATE provider_turn_executions SET phase = CASE WHEN phase = 'interrupt_ambiguous' THEN phase ELSE 'recovery_required' END, updated_at = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ?")
        .bind(chrono::Utc::now().to_rfc3339()).bind(&session.public.room_id)
        .bind(&session.public.session_id).bind(i64::try_from(session.turn_generation).map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?)
        .execute(&mut **tx).await?;
    session.public.recovery_required = true;
    if changed {
        session.public.updated_at = chrono::Utc::now();
        crate::agent_lifecycle::save_session(tx, session).await?;
    }
    release_inputs(tx, session).await?;
    if changed {
        let event = crate::room_turns::support::session_state_event(tx, session).await?;
        progress(tx, &session.public.room_id, vec![event]).await
    } else {
        Ok(crate::AgentTurnCommit {
            events: Vec::new(),
            next_assignments: Vec::new(),
        })
    }
}

async fn progress(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    mut events: Vec<agentsassemble_domain::RoomEvent>,
) -> Result<crate::AgentTurnCommit, PersistenceError> {
    let (room, settings) = load_room_with_settings(tx, room_id).await?;
    let mut commit = if room.status == agentsassemble_domain::RoomStatus::Active {
        crate::room_turns::assign_pending_in(tx, &room, &settings).await?
    } else {
        crate::AgentTurnCommit {
            events: Vec::new(),
            next_assignments: Vec::new(),
        }
    };
    events.append(&mut commit.events);
    commit.events = events;
    Ok(commit)
}

/// Repair pre-receipt executions before runtime reconciliation/network admission.
/// Persist assignments for the existing startup execution reconciliation scan.
pub(crate) async fn repair_startup(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    loop {
        let mut tx = pool.begin().await?;
        let row: Option<(String, String, String)> = sqlx::query_as(
            "SELECT e.room_id, e.session_id, e.phase FROM provider_turn_executions e JOIN agent_sessions s USING (room_id, session_id) WHERE e.released_input_ids IS NULL AND e.turn_generation = json_extract(s.session_json, '$.turn_generation') AND (e.phase IN ('recovery_required', 'interrupt_ambiguous') OR (e.phase = 'interrupted' AND EXISTS (SELECT 1 FROM room_events r WHERE r.room_id = e.room_id AND json_extract(r.event_json, '$.type') = 'turn_finished' AND json_extract(r.event_json, '$.turn_id') = e.turn_id AND json_extract(r.event_json, '$.reason_code') = 'operator_stop'))) ORDER BY e.room_id, e.session_id LIMIT 1",
        ).fetch_optional(&mut *tx).await?;
        let Some((room_id, session_id, phase)) = row else {
            return Ok(());
        };
        let mut session =
            crate::agent_lifecycle::load_session(&mut tx, &room_id, &session_id).await?;
        if phase == "interrupted" {
            repair_confirmed_stop(&mut tx, &mut session).await?;
            progress(&mut tx, &room_id, Vec::new()).await?;
        } else {
            quarantine(&mut tx, &mut session).await?;
        }
        tx.commit().await?;
    }
}

async fn repair_confirmed_stop(
    tx: &mut Transaction<'_, Sqlite>,
    session: &mut DurableAgentSession,
) -> Result<(), PersistenceError> {
    if !session.inflight_inputs.is_empty() {
        return release_inputs(tx, session).await;
    }
    let execution = crate::provider_turn_execution::load_execution_in(
        tx,
        &session.public.room_id,
        &session.public.session_id,
        session.turn_generation,
    )
    .await?;
    let through: i64 = sqlx::query_scalar("SELECT json_extract(event_json, '$.provider_context_up_to_seq') FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.type') = 'turn_started' AND json_extract(event_json, '$.turn_id') = ?")
        .bind(&session.public.room_id).bind(&execution.turn_id).fetch_one(&mut **tx).await?;
    let mut retained = session.clone();
    for input in &session.pending_inputs {
        let event = load_event(tx, &session.public.room_id, &input.event_id)
            .await?
            .ok_or_else(crate::agent_lifecycle::invalid_turn_queue)?;
        if event.seq <= through {
            retained.inflight_inputs.push(input.clone());
        }
    }
    release_inputs(tx, &retained).await?;
    let released = released_inputs(tx, session).await?;
    session
        .pending_inputs
        .retain(|input| !released.contains(&input.event_id));
    crate::agent_lifecycle::save_session(tx, session).await
}
