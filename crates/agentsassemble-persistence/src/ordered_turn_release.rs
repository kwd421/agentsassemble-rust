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
    Ok(crate::provider_turn_execution::load_execution_in(
        tx,
        &session.public.room_id,
        &session.public.session_id,
        session.turn_generation,
    )
    .await?
    .phase
        == crate::ProviderTurnExecutionPhase::RecoveryRequired)
}

/// NULL means no release attempt; an array is the receipt for this generation.
/// A receipt only contains inputs actually queued to another speaker, never direct targets.
pub(crate) async fn release_inputs(
    tx: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let receipt: Option<String> = sqlx::query_scalar(
        "SELECT released_input_ids FROM provider_turn_executions WHERE room_id = ? AND session_id = ? AND turn_generation = ?",
    ).bind(&session.public.room_id).bind(&session.public.session_id)
        .bind(i64::try_from(session.turn_generation).map_err(|_| crate::agent_lifecycle::invalid_turn_queue())?)
        .fetch_one(&mut **tx).await?;
    if receipt.is_some() {
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
