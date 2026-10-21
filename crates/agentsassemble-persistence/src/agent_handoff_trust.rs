//! The people whose AIs may hand an agent work that full access runs unasked.
//! A private field of the durable session, empty by default.
use crate::{PersistenceError, SqliteStore};

const MAX_TRUSTED_OWNERS: usize = 50;

impl SqliteStore {
    /// `None` when the room has no such agent.
    ///
    /// # Errors
    /// Fails when the session cannot be read.
    pub async fn agent_trusted_ai_owners(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Option<Vec<String>>, PersistenceError> {
        let Some(owners) = sqlx::query_scalar::<_, String>(
            "SELECT COALESCE(json_extract(session_json, '$.trusted_ai_owners'), '[]') FROM agent_sessions WHERE room_id = ? AND session_id = ?",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?
        else {
            return Ok(None);
        };
        Ok(Some(serde_json::from_str(&owners)?))
    }

    /// Only people in the room, never the agent's own owner (already
    /// trusted) and no one twice. Returns false when the room has no such agent.
    ///
    /// # Errors
    /// Rejects anyone who is not a person in the room, and storage failures.
    pub async fn set_agent_trusted_ai_owners(
        &self,
        room_id: &str,
        session_id: &str,
        owners: &[String],
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let Some(agent_owner) = sqlx::query_scalar::<_, String>(
            "SELECT COALESCE(json_extract(p.participant_json, '$.owner_id'), '') FROM agent_sessions s JOIN participants p ON p.room_id = s.room_id AND p.participant_id = json_extract(s.session_json, '$.participant_id') WHERE s.room_id = ? AND s.session_id = ?",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_optional(&mut *tx)
        .await?
        else {
            return Ok(false);
        };
        let mut kept: Vec<String> = Vec::new();
        for owner in owners {
            if *owner == agent_owner || kept.contains(owner) {
                continue;
            }
            let person = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM participants WHERE room_id = ? AND participant_id = ? AND json_extract(participant_json, '$.participant_type') = 'human'",
            )
            .bind(room_id)
            .bind(owner)
            .fetch_one(&mut *tx)
            .await?;
            if person != 1 || kept.len() == MAX_TRUSTED_OWNERS {
                return Err(PersistenceError::CommandRejected {
                    code: "bad_request".into(),
                    message: "Only people in this room can be trusted.".to_owned(),
                });
            }
            kept.push(owner.clone());
        }
        let updated = if kept.is_empty() {
            sqlx::query("UPDATE agent_sessions SET session_json = json_remove(session_json, '$.trusted_ai_owners') WHERE room_id = ? AND session_id = ?")
                .bind(room_id)
                .bind(session_id)
                .execute(&mut *tx)
                .await?
        } else {
            sqlx::query("UPDATE agent_sessions SET session_json = json_set(session_json, '$.trusted_ai_owners', json(?)) WHERE room_id = ? AND session_id = ?")
                .bind(serde_json::to_string(&kept)?)
                .bind(room_id)
                .bind(session_id)
                .execute(&mut *tx)
                .await?
        };
        tx.commit().await?;
        Ok(updated.rows_affected() == 1)
    }
}
