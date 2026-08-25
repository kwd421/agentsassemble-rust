use agentsassemble_domain::{AuthenticatedPrincipal, canonical_payload_hash};
use serde_json::Value;

use crate::{
    PersistenceError, RuntimeReconciliationCandidate, SqliteStore,
    agent_reconciliation::{invalid_stored_authority, load_candidate, stale_candidate},
};

impl SqliteStore {
    /// Loads one exact current-supervisor candidate for a lifecycle command replay.
    ///
    /// # Errors
    ///
    /// Returns a command conflict, previous-supervisor, or stored-authority failure when the
    /// command does not own a live-recoverable durable operation.
    pub async fn load_lifecycle_reconciliation_candidate(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<RuntimeReconciliationCandidate, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let session_id = sqlx::query_scalar::<_, String>(
            "SELECT session_id FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ? AND action = ? AND payload_hash = ? AND status = 'pending'",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(action)
        .bind(canonical_payload_hash(payload))
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PersistenceError::CommandConflict)?;
        let candidate = load_candidate(&mut transaction, &principal.room_id, &session_id)
            .await?
            .ok_or_else(stale_candidate)?;
        let Some(reservation) = &candidate.reservation else {
            return Err(invalid_stored_authority());
        };
        if !same_principal_identity(&reservation.principal, principal)
            || reservation.request_id != request_id
            || reservation.action != action
            || reservation.payload != *payload
            || candidate.session.lifecycle_intent_status != "unconfirmed"
        {
            return Err(PersistenceError::CommandConflict);
        }
        if reservation.supervisor_generation != self.runtime_generation() {
            return Err(previous_supervisor_effect());
        }
        transaction.commit().await?;
        Ok(candidate)
    }
}

fn same_principal_identity(
    stored: &AuthenticatedPrincipal,
    current: &AuthenticatedPrincipal,
) -> bool {
    stored.principal_id == current.principal_id
        && stored.participant_id == current.participant_id
        && stored.room_id == current.room_id
        && stored.client_kind == current.client_kind
        && stored.invite_scope == current.invite_scope
        && stored.is_operator == current.is_operator
}

fn previous_supervisor_effect() -> PersistenceError {
    PersistenceError::CommandUnresolved {
        code: "runtime_effect_unconfirmed",
        message: "The original provider effect belongs to a previous server runtime and awaits server-owned reconciliation.".to_owned(),
    }
}
