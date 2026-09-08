//! Request cancellation follows existing session transitions, never a periodic authority scan.
use agentsassemble_domain::AgentSession;
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError, SqliteStore, provider_request_authority::require_pending_authority_in,
    provider_request_completion::close_in,
};

impl SqliteStore {
    /// Reads the durable pending set for event-driven live delivery reconciliation.
    ///
    /// # Errors
    /// Propagates storage failures and invalid stored identities.
    pub async fn pending_provider_request_ids(
        &self,
        room_id: &str,
    ) -> Result<Vec<uuid::Uuid>, PersistenceError> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT request_id FROM provider_requests WHERE room_id=? AND state IN ('open','resolving')",
        ).bind(room_id).fetch_all(&self.pool).await?;
        ids.into_iter()
            .map(|id| {
                uuid::Uuid::parse_str(&id).map_err(|_| {
                    crate::provider_requests::rejected(
                        "invalid_state",
                        "Stored request identity is invalid.",
                    )
                })
            })
            .collect()
    }

    /// Fails pre-restart live requests before accepting network traffic.
    ///
    /// # Errors
    /// Propagates storage failure; lost live answers can never be reconstructed as success.
    pub async fn fail_provider_requests_before_admission(&self) -> Result<(), PersistenceError> {
        loop {
            let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
            let rows = sqlx::query(
                "SELECT * FROM provider_requests WHERE state IN ('open','resolving') LIMIT 64",
            )
            .fetch_all(&mut *tx)
            .await?;
            if rows.is_empty() {
                tx.commit().await?;
                return Ok(());
            }
            for row in rows {
                close_in(&mut tx, &row, "failed").await?;
            }
            tx.commit().await?;
        }
    }
}

// A participant mutation already owns the authorization change in this transaction.
pub(crate) async fn cancel_participant_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    let rows = sqlx::query("SELECT request.* FROM provider_requests request WHERE request.room_id=? AND request.state IN ('open','resolving') AND (request.owner_id=? OR EXISTS(SELECT 1 FROM provider_turn_executions execution WHERE execution.room_id=request.room_id AND execution.session_id=request.session_id AND execution.turn_generation=request.turn_generation AND execution.participant_id=?))")
        .bind(room_id).bind(participant_id).bind(participant_id).fetch_all(&mut **tx).await?;
    for row in rows {
        close_in(tx, &row, "cancelled").await?;
    }
    Ok(())
}

// Called before the existing canonical session event. Its publication owner catches up all
// committed events in sequence, so the private close event is published in that same batch.
// The partial pending-session index bounds this work to at most one request per transition.
pub(crate) async fn reconcile_session_in(
    tx: &mut Transaction<'_, Sqlite>,
    session: &AgentSession,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let rows = sqlx::query("SELECT * FROM provider_requests WHERE room_id=? AND session_id=? AND state IN ('open','resolving')")
        .bind(&session.room_id).bind(&session.session_id)
        .fetch_all(&mut **tx).await?;
    if rows.is_empty() {
        return Ok(());
    }
    let session =
        crate::agent_lifecycle::load_session(tx, &session.room_id, &session.session_id).await?;
    for row in rows {
        let state = if row.get::<i64, _>("expires_at") <= now.timestamp_millis() {
            Some("expired")
        } else {
            match require_pending_authority_in(tx, &session, &row, now).await {
                Ok(()) => None,
                Err(
                    PersistenceError::RoomMissing
                    | PersistenceError::ParticipantMissing
                    | PersistenceError::CommandRejected {
                        code:
                            "stale_provider_turn"
                            | "bridge_connection_replaced"
                            | "session_revoked"
                            | "permission_denied"
                            | "room_inactive",
                        ..
                    },
                ) => Some("cancelled"),
                Err(error) => return Err(error),
            }
        };
        if let Some(state) = state {
            close_in(tx, &row, state).await?;
        }
    }
    Ok(())
}
