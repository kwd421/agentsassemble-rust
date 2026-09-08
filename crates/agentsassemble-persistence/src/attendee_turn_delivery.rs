use crate::{
    AttendeeConnectionAuthorization, PersistenceError, ProviderTurnAssignmentEnvelope,
    ProviderTurnExecutionPhase, ProviderTurnStartAuthority, SqliteStore,
    attendee_invites::rejected,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Private assignment for this external runtime only; contains no host configuration or paths.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeTurnDelivery {
    pub authority: ProviderTurnStartAuthority,
    pub input: ProviderTurnAssignmentEnvelope,
    pub input_up_to_seq: i64,
    pub provider_turn_id: String,
    /// A previous delivery may already have caused I/O. The external runtime resolves that custody.
    pub resume: bool,
}

impl SqliteStore {
    /// Delivers or recovers one exact assignment with connection authorization in the same transaction.
    ///
    /// # Errors
    /// Rejects unready/replaced/revoked custody, effect-bearing turns and corrupt assignment state.
    pub async fn deliver_attendee_turn(
        &self,
        connection: &AttendeeConnectionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<Option<AttendeeTurnDelivery>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, connection, now).await?;
        let ready: bool = sqlx::query_scalar("SELECT state='ready' FROM attendee_connections WHERE session_fingerprint=? AND connection_id=?")
            .bind(connection.session.fingerprint.as_slice()).bind(connection.connection_id.to_string()).fetch_one(&mut *tx).await?;
        if !ready {
            return Err(rejected(
                "bridge_not_ready",
                "The external connection is not ready for turn delivery.",
            ));
        }
        let principal = connection.session.principal();
        let Some(candidate) = crate::provider_turn_reconciliation::load_active_candidate_in(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?
        else {
            tx.commit().await?;
            return Ok(None);
        };
        let execution = &candidate.execution;
        if candidate.effect.is_some()
            || !matches!(
                execution.phase,
                ProviderTurnExecutionPhase::Assigned
                    | ProviderTurnExecutionPhase::StartDispatching
                    | ProviderTurnExecutionPhase::Running
            )
        {
            return Err(rejected(
                "operation_in_progress",
                "The external turn requires its pending lifecycle reconciliation.",
            ));
        }
        let input = crate::provider_turn_reconciliation::load_assignment_in(
            &mut tx,
            &candidate.session,
            execution,
        )
        .await?;
        let resume = execution.phase != ProviderTurnExecutionPhase::Assigned;
        let authority = if resume {
            execution.clone().into()
        } else {
            crate::provider_turn_execution::authorize_start_in(
                &mut tx,
                &execution.room_id,
                &execution.session_id,
                execution.turn_generation,
                &execution.turn_id,
            )
            .await?
        };
        tx.commit().await?;
        Ok(Some(AttendeeTurnDelivery {
            authority,
            input,
            input_up_to_seq: candidate.session.input_up_to_seq,
            provider_turn_id: execution.provider_turn_id.clone(),
            resume,
        }))
    }

    /// Records an exact external start; a lost acknowledgement may be retried by the current connection.
    ///
    /// # Errors
    /// Rejects replaced/revoked connections and changed provider/runtime/execution identities.
    pub async fn record_attendee_turn_started(
        &self,
        connection: &AttendeeConnectionAuthorization,
        authority: &ProviderTurnStartAuthority,
        provider_turn_id: &str,
        now: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, connection, now).await?;
        let principal = connection.session.principal();
        let session = crate::agent_lifecycle::load_session(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        if authority.room_id != principal.room_id
            || authority.session_id != session.public.session_id
        {
            return Err(rejected(
                "permission_denied",
                "The start report belongs to a different attendee.",
            ));
        }
        let execution = crate::provider_turn_execution::load_execution_in(
            &mut tx,
            &authority.room_id,
            &authority.session_id,
            authority.turn_generation,
        )
        .await?;
        let replay = execution.phase == ProviderTurnExecutionPhase::Running
            && execution.provider_turn_id == provider_turn_id
            && ProviderTurnStartAuthority::from(execution) == *authority;
        if !replay {
            crate::provider_turn_execution::mark_running_in(&mut tx, authority, provider_turn_id)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
