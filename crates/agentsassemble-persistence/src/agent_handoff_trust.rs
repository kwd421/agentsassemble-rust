//! Whether an agent's owner lets other people's AIs hand it work that full
//! access runs unasked. A private field of the durable session, off by default.
use crate::{PersistenceError, SqliteStore};

impl SqliteStore {
    /// `None` when the room has no such agent.
    ///
    /// # Errors
    /// Fails when the session cannot be read.
    pub async fn agent_trusts_ai_handoffs(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Option<bool>, PersistenceError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(json_extract(session_json, '$.trusts_ai_handoffs'), 0) FROM agent_sessions WHERE room_id = ? AND session_id = ?",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        .map(|trusted| trusted == 1))
    }

    /// Returns false when the room has no such agent.
    ///
    /// # Errors
    /// Fails when the session cannot be written.
    pub async fn set_agent_trusts_ai_handoffs(
        &self,
        room_id: &str,
        session_id: &str,
        trusted: bool,
    ) -> Result<bool, PersistenceError> {
        let sql = if trusted {
            "UPDATE agent_sessions SET session_json = json_set(session_json, '$.trusts_ai_handoffs', json('true')) WHERE room_id = ? AND session_id = ?"
        } else {
            "UPDATE agent_sessions SET session_json = json_remove(session_json, '$.trusts_ai_handoffs') WHERE room_id = ? AND session_id = ?"
        };
        let updated = sqlx::query(sql)
            .bind(room_id)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(updated.rows_affected() == 1)
    }
}
