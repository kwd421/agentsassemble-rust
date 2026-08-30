use sqlx::Row;

use crate::{
    PersistenceError, RuntimeReconciliationCandidate, SqliteStore,
    agent_reconciliation::load_candidate,
};

const RECONCILIATION_SCAN_LIMIT: u8 = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeReconciliationCursor {
    room_id: String,
    session_id: String,
}

#[derive(Debug)]
pub struct RuntimeReconciliationPage {
    pub candidates: Vec<RuntimeReconciliationCandidate>,
    pub next_cursor: Option<RuntimeReconciliationCursor>,
}

impl SqliteStore {
    /// Scans one bounded page and returns exact durable nonterminal lifecycle candidates.
    ///
    /// Candidate selection stays at the pending-reservation owner: ordinary Agent Sessions and
    /// terminal reservation history do not incur periodic reconciliation reads, while every
    /// selected row still enters exact session/reservation authority validation.
    ///
    /// # Errors
    ///
    /// Returns a persistence or stored-authority failure.
    pub async fn load_runtime_reconciliation_page(
        &self,
        cursor: Option<&RuntimeReconciliationCursor>,
    ) -> Result<RuntimeReconciliationPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let room_id = cursor.map(|cursor| cursor.room_id.as_str());
        let session_id = cursor.map(|cursor| cursor.session_id.as_str());
        let rows = sqlx::query(
            "SELECT reservation.room_id, reservation.session_id \
             FROM lifecycle_command_reservations AS reservation \
             JOIN agent_sessions AS sessions ON sessions.room_id = reservation.room_id \
               AND sessions.session_id = reservation.session_id \
             WHERE reservation.status = 'pending' \
             AND (? IS NULL OR reservation.room_id > ? OR (reservation.room_id = ? AND reservation.session_id > ?)) \
             AND NOT EXISTS (SELECT 1 FROM provider_turn_executions AS execution \
               WHERE execution.room_id = reservation.room_id AND execution.session_id = reservation.session_id \
               AND execution.phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', \
                 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')) \
             ORDER BY reservation.room_id, reservation.session_id LIMIT ?",
        )
        .bind(room_id)
        .bind(room_id)
        .bind(room_id)
        .bind(session_id)
        .bind(i64::from(RECONCILIATION_SCAN_LIMIT))
        .fetch_all(&mut *transaction)
        .await?;
        let next_cursor = if rows.len() == usize::from(RECONCILIATION_SCAN_LIMIT) {
            rows.last().map(|row| RuntimeReconciliationCursor {
                room_id: row.get("room_id"),
                session_id: row.get("session_id"),
            })
        } else {
            None
        };
        let mut candidates = Vec::new();
        for row in rows {
            let room_id = row.get::<String, _>("room_id");
            let session_id = row.get::<String, _>("session_id");
            let Some(candidate) = load_candidate(&mut transaction, &room_id, &session_id).await?
            else {
                continue;
            };
            if matches!(
                candidate.session.lifecycle_intent_status.as_str(),
                "prepared" | "effect_inflight" | "unconfirmed" | "effect_applied"
            ) && candidate.reservation.is_some()
            {
                candidates.push(candidate);
            }
        }
        transaction.commit().await?;
        Ok(RuntimeReconciliationPage {
            candidates,
            next_cursor,
        })
    }
}
