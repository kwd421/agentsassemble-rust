use chrono::Utc;
use sqlx::{Sqlite, Transaction};

use crate::{
    AgentTurnCommit, PersistenceError, ProviderTurnExecutionPhase, SqliteStore,
    agent_lifecycle::{load_session, save_session},
    provider_turn_effect::{
        ProviderTurnEffectPhase, ProviderTurnInterruptEffect, canonical_now, generation_i64,
        invalid_effect, load_effect_in, require_exact_effect, stale_effect,
    },
    provider_turn_execution::load_execution_in,
    room_turns::{
        assign_pending_in,
        support::{
            clear_active_turn_fields, load_active_room, session_state_event, turn_finished_event,
        },
    },
    turn_authority::active_turn_authority,
    turn_queue::merge_room_inputs,
};

impl SqliteStore {
    /// Finalizes one proved-quiescent interrupted turn while retaining its runtime.
    ///
    /// # Errors
    ///
    /// Rejects stale execution, effect, session, or runtime custody authority.
    pub async fn finalize_interrupted_turn_retained(
        &self,
        expected: &ProviderTurnInterruptEffect,
    ) -> Result<AgentTurnCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (room, settings) = load_active_room(&mut transaction, &expected.room_id).await?;
        let mut session =
            load_session(&mut transaction, &expected.room_id, &expected.session_id).await?;
        if !active_turn_authority(&session).map_err(|_| invalid_effect())?
            || session.public.participant_id != expected.participant_id
            || session.public.active_turn_id != expected.turn_id
            || session.turn_generation != expected.turn_generation
            || session.runtime_handle_id != expected.runtime_handle_id
            || session.runtime_owner_id != expected.runtime_owner_id
            || session.runtime_lease_token != expected.runtime_lease_token
        {
            return Err(stale_effect());
        }
        terminalize_retained_interrupt(&mut transaction, expected).await?;

        let turn_id = session.public.active_turn_id.clone();
        let finished = turn_finished_event(
            &mut transaction,
            &session,
            &turn_id,
            "interrupted",
            None,
            Some("participant_muted"),
        )
        .await?;
        session.pending_inputs = merge_room_inputs(
            session
                .inflight_inputs
                .iter()
                .chain(&session.pending_inputs),
        )
        .map_err(|_| invalid_effect())?;
        session.inflight_inputs.clear();
        "attached".clone_into(&mut session.public.status);
        "idle".clone_into(&mut session.public.runtime_status);
        session.public.turn_phase.clear();
        session.public.active_turn_id.clear();
        session.public.last_error.clear();
        session.public.last_error_code.clear();
        session.public.recovery_required = false;
        clear_active_turn_fields(&mut session);
        session.public.updated_at = Utc::now();
        save_session(&mut transaction, &session).await?;
        let state = session_state_event(&mut transaction, &session).await?;
        let scheduled = assign_pending_in(&mut transaction, &room, &settings).await?;
        transaction.commit().await?;
        let mut events = vec![finished, state];
        events.extend(scheduled.events);
        Ok(AgentTurnCommit {
            events,
            next_assignments: scheduled.next_assignments,
        })
    }
}

async fn terminalize_retained_interrupt(
    transaction: &mut Transaction<'_, Sqlite>,
    expected: &ProviderTurnInterruptEffect,
) -> Result<(), PersistenceError> {
    let execution = load_execution_in(
        transaction,
        &expected.room_id,
        &expected.session_id,
        expected.turn_generation,
    )
    .await?;
    if execution.phase != ProviderTurnExecutionPhase::Quiescing
        || execution.execution_id != expected.execution_id
        || execution.participant_id != expected.participant_id
        || execution.turn_id != expected.turn_id
        || execution.runtime_handle_id != expected.runtime_handle_id
        || execution.runtime_owner_id != expected.runtime_owner_id
        || execution.runtime_lease_token != expected.runtime_lease_token
        || execution.requeue_finalized
    {
        return Err(stale_effect());
    }
    let effect = load_effect_in(
        transaction,
        &expected.room_id,
        &expected.session_id,
        expected.turn_generation,
    )
    .await?;
    require_exact_effect(expected, &effect)?;
    if effect.phase != ProviderTurnEffectPhase::IssuedWaitingQuiescence {
        return Err(stale_effect());
    }
    let now = canonical_now();
    let effect_changed = sqlx::query(
        "UPDATE provider_turn_effects SET phase = 'finalized', claim_expires_at = NULL, \
         updated_at = ? WHERE room_id = ? AND effect_id = ? \
         AND phase = 'issued_waiting_quiescence' AND dispatch_nonce = ?",
    )
    .bind(&now)
    .bind(&effect.room_id)
    .bind(&effect.effect_id)
    .bind(&effect.dispatch_nonce)
    .execute(&mut **transaction)
    .await?;
    let execution_changed = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'interrupted', \
         requeue_finalized = 1, updated_at = ? WHERE room_id = ? AND session_id = ? \
         AND turn_generation = ? AND execution_id = ? AND phase = 'quiescing' \
         AND requeue_finalized = 0 AND runtime_handle_id = ? \
         AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(&now)
    .bind(&execution.room_id)
    .bind(&execution.session_id)
    .bind(generation_i64(execution.turn_generation)?)
    .bind(&execution.execution_id)
    .bind(&execution.runtime_handle_id)
    .bind(&execution.runtime_owner_id)
    .bind(&execution.runtime_lease_token)
    .execute(&mut **transaction)
    .await?;
    if effect_changed.rows_affected() != 1 || execution_changed.rows_affected() != 1 {
        return Err(stale_effect());
    }
    Ok(())
}
