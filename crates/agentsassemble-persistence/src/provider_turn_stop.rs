use agentsassemble_domain::{DurableAgentSession, RoomEvent};
use chrono::{SecondsFormat, Utc};
use sqlx::{Sqlite, Transaction};

use crate::{
    PersistenceError, ProviderTurnEffectPhase, provider_turn_effect::load_optional_effect_in,
    provider_turn_execution::load_execution_in, room_turns::support::turn_finished_event,
    turn_authority::active_turn_authority,
};

/// Terminalizes the exact active provider execution after its runtime is proved stopped.
pub(crate) async fn terminalize_confirmed_stop_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<Option<RoomEvent>, PersistenceError> {
    if !active_turn_authority(session).map_err(|_| invalid_stop_turn())? {
        return Ok(None);
    }
    let execution = load_execution_in(
        transaction,
        &session.public.room_id,
        &session.public.session_id,
        session.turn_generation,
    )
    .await?;
    if !execution.phase.is_blocking()
        || execution.requeue_finalized
        || execution.participant_id != session.public.participant_id
        || execution.turn_id != session.public.active_turn_id
        || execution.runtime_handle_id != session.runtime_handle_id
        || execution.runtime_owner_id != session.runtime_owner_id
        || execution.runtime_lease_token != session.runtime_lease_token
    {
        return Err(invalid_stop_turn());
    }
    let effect = load_optional_effect_in(
        transaction,
        &execution.room_id,
        &execution.session_id,
        execution.turn_generation,
    )
    .await?;
    if effect.as_ref().is_some_and(|effect| {
        effect.phase == ProviderTurnEffectPhase::Finalized
            || effect.execution_id != execution.execution_id
            || effect.participant_id != execution.participant_id
            || effect.turn_id != execution.turn_id
            || effect.start_dispatch_nonce != execution.start_dispatch_nonce
            || effect.runtime_handle_id != execution.runtime_handle_id
            || effect.runtime_owner_id != execution.runtime_owner_id
            || effect.runtime_lease_token != execution.runtime_lease_token
    }) {
        return Err(invalid_stop_turn());
    }
    let now = Utc::now().to_rfc3339_opts(SecondsFormat::Micros, true);
    let execution_changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'interrupted', requeue_finalized = 1, \
         updated_at = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
         AND execution_id = ? AND phase = ? AND requeue_finalized = 0 \
         AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(&now)
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
        return Err(stale_stop_turn());
    }
    if let Some(effect) = effect {
        let effect_changed = sqlx::query(
            "UPDATE provider_turn_effects SET phase = 'finalized', claim_expires_at = NULL, \
             updated_at = ? WHERE room_id = ? AND effect_id = ? AND phase = ? \
             AND dispatch_nonce = ?",
        )
        .bind(&now)
        .bind(&effect.room_id)
        .bind(&effect.effect_id)
        .bind(effect.phase.as_str())
        .bind(&effect.dispatch_nonce)
        .execute(&mut **transaction)
        .await?;
        if effect_changed.rows_affected() != 1 {
            return Err(stale_stop_turn());
        }
    }
    turn_finished_event(
        transaction,
        session,
        &execution.turn_id,
        "interrupted",
        None,
        Some("operator_stop"),
    )
    .await
    .map(Some)
}

fn generation_i64(generation: u64) -> Result<i64, PersistenceError> {
    i64::try_from(generation).map_err(|_| invalid_stop_turn())
}

fn invalid_stop_turn() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stored_provider_stop_turn_invalid",
        message: "Stored provider turn authority is invalid during confirmed stop.".to_owned(),
    }
}

fn stale_stop_turn() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_provider_stop_turn",
        message: "Provider turn authority changed before confirmed stop committed.".to_owned(),
    }
}
