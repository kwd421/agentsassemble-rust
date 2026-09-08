use super::{
    ProviderTurnStartAuthority, canonical_now, generation_i64, invalid_execution,
    load_execution_in, stale_execution,
};
use crate::{
    PersistenceError, SqliteStore, agent_lifecycle::load_session,
    room_turns::support::load_participant, turn_authority::active_turn_authority,
};
use agentsassemble_domain::{ParticipantStatus, is_provider_turn_id};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

impl SqliteStore {
    /// Consumes the exact durable start authorization before provider I/O.
    ///
    /// # Errors
    /// Rejects stale, muted, quarantined, or already consumed assignments.
    pub async fn authorize_provider_turn_start(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
        turn_id: &str,
    ) -> Result<ProviderTurnStartAuthority, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let authority = authorize_start_in(
            &mut transaction,
            room_id,
            session_id,
            turn_generation,
            turn_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(authority)
    }

    /// Marks a started provider turn under its exact dispatch and runtime fence.
    ///
    /// # Errors
    /// Rejects stale dispatch or malformed provider turn identity.
    pub async fn mark_provider_turn_running(
        &self,
        authority: &ProviderTurnStartAuthority,
        provider_turn_id: &str,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        mark_running_in(&mut transaction, authority, provider_turn_id).await?;
        transaction.commit().await?;
        Ok(())
    }
}

pub(crate) async fn authorize_start_in(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    turn_generation: u64,
    turn_id: &str,
) -> Result<ProviderTurnStartAuthority, PersistenceError> {
    let session = load_session(transaction, room_id, session_id).await?;
    let participant =
        load_participant(transaction, room_id, &session.public.participant_id).await?;
    if participant.room_id != room_id
        || participant.participant_id != session.public.participant_id
        || participant.status != ParticipantStatus::Joined
        || participant.muted
        || session.public.active_turn_id != turn_id
        || session.turn_generation != turn_generation
        || !active_turn_authority(&session).map_err(|_| invalid_execution())?
    {
        return Err(stale_execution());
    }
    let nonce = Uuid::new_v4().to_string();
    let updated = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'start_dispatching', \
             start_dispatch_nonce = ?, updated_at = ? \
             WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND turn_id = ? AND participant_id = ? AND phase = 'assigned' \
             AND runtime_handle_id = ? AND runtime_owner_id = ? AND runtime_lease_token = ? \
             AND NOT EXISTS (SELECT 1 FROM provider_turn_effects effect \
               WHERE effect.room_id = provider_turn_executions.room_id \
               AND effect.session_id = provider_turn_executions.session_id \
               AND effect.turn_generation = provider_turn_executions.turn_generation \
               AND effect.phase != 'finalized')",
    )
    .bind(&nonce)
    .bind(canonical_now())
    .bind(room_id)
    .bind(session_id)
    .bind(generation_i64(turn_generation)?)
    .bind(turn_id)
    .bind(&session.public.participant_id)
    .bind(&session.runtime_handle_id)
    .bind(&session.runtime_owner_id)
    .bind(&session.runtime_lease_token)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(stale_execution());
    }
    let execution = load_execution_in(transaction, room_id, session_id, turn_generation).await?;
    Ok(execution.into())
}

pub(crate) async fn mark_running_in(
    transaction: &mut Transaction<'_, Sqlite>,
    authority: &ProviderTurnStartAuthority,
    provider_turn_id: &str,
) -> Result<(), PersistenceError> {
    if !is_provider_turn_id(provider_turn_id) {
        return Err(invalid_execution());
    }
    let updated = sqlx::query(
        "UPDATE provider_turn_executions SET phase = 'running', provider_turn_id = ?, \
             updated_at = ? WHERE room_id = ? AND session_id = ? AND turn_generation = ? \
             AND execution_id = ? AND turn_id = ? AND phase = 'start_dispatching' \
             AND start_dispatch_nonce = ? AND runtime_handle_id = ? \
             AND runtime_owner_id = ? AND runtime_lease_token = ?",
    )
    .bind(provider_turn_id)
    .bind(canonical_now())
    .bind(&authority.room_id)
    .bind(&authority.session_id)
    .bind(generation_i64(authority.turn_generation)?)
    .bind(&authority.execution_id)
    .bind(&authority.turn_id)
    .bind(&authority.start_dispatch_nonce)
    .bind(&authority.runtime_handle_id)
    .bind(&authority.runtime_owner_id)
    .bind(&authority.runtime_lease_token)
    .execute(&mut **transaction)
    .await?;
    if updated.rows_affected() != 1 {
        return Err(stale_execution());
    }
    Ok(())
}

impl From<super::ProviderTurnExecution> for ProviderTurnStartAuthority {
    fn from(execution: super::ProviderTurnExecution) -> Self {
        Self {
            room_id: execution.room_id,
            session_id: execution.session_id,
            turn_generation: execution.turn_generation,
            execution_id: execution.execution_id,
            turn_id: execution.turn_id,
            runtime_handle_id: execution.runtime_handle_id,
            runtime_owner_id: execution.runtime_owner_id,
            runtime_lease_token: execution.runtime_lease_token,
            start_dispatch_nonce: execution.start_dispatch_nonce,
        }
    }
}
