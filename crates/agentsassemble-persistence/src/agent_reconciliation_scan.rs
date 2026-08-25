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
    /// Scans one bounded page and returns only exact durable unconfirmed lifecycle candidates.
    ///
    /// The cursor advances over every Agent Session, including sessions that do not currently
    /// need reconciliation, so a large inactive prefix cannot starve later work.
    ///
    /// # Errors
    ///
    /// Returns a persistence or stored-authority failure.
    pub async fn load_unconfirmed_runtime_reconciliation_page(
        &self,
        cursor: Option<&RuntimeReconciliationCursor>,
    ) -> Result<RuntimeReconciliationPage, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let room_id = cursor.map(|cursor| cursor.room_id.as_str());
        let session_id = cursor.map(|cursor| cursor.session_id.as_str());
        let rows = sqlx::query(
            "SELECT room_id, session_id FROM agent_sessions WHERE (? IS NULL OR room_id > ? OR (room_id = ? AND session_id > ?)) ORDER BY room_id, session_id LIMIT ?",
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
            if candidate.session.lifecycle_intent_status == "unconfirmed"
                && candidate.reservation.is_some()
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
