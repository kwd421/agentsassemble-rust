//! Running model-use totals of each API AI, for its owner's usage line. They
//! are statistics, not room content, so backups leave them out.
use agentsassemble_domain::{AgentTurnUsage, AgentUsageTotals};
use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS agent_usage (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turns INTEGER NOT NULL, steps INTEGER NOT NULL, prompt_tokens INTEGER NOT NULL, cached_prompt_tokens INTEGER NOT NULL, completion_tokens INTEGER NOT NULL, last_prompt_tokens INTEGER NOT NULL, context_tokens INTEGER NOT NULL, model_ms INTEGER NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE) STRICT";

pub(crate) const TABLES: &[crate::schema::TableDefinition] = &[crate::schema::TableDefinition {
    name: "agent_usage",
    ddl: DDL,
    infrastructure: false,
}];

/// One turn never counts more than this per field, so a server reporting
/// nonsense cannot overflow the totals.
const MAX_TURN_VALUE: u64 = 1_000_000_000_000;

fn stored(value: u64) -> i64 {
    i64::try_from(value.min(MAX_TURN_VALUE)).unwrap_or(0)
}

fn loaded(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

impl SqliteStore {
    /// Adds one finished turn to the AI's totals. The last prompt size and the
    /// context window replace the earlier ones when the turn reported them.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn record_agent_usage(
        &self,
        room_id: &str,
        session_id: &str,
        usage: &AgentTurnUsage,
    ) -> Result<(), PersistenceError> {
        sqlx::query("INSERT INTO agent_usage(room_id, session_id, turns, steps, prompt_tokens, cached_prompt_tokens, completion_tokens, last_prompt_tokens, context_tokens, model_ms) SELECT room_id, session_id, 1, ?, ?, ?, ?, ?, ?, ? FROM agent_sessions WHERE room_id = ? AND session_id = ? ON CONFLICT(room_id, session_id) DO UPDATE SET turns = turns + 1, steps = steps + excluded.steps, prompt_tokens = prompt_tokens + excluded.prompt_tokens, cached_prompt_tokens = cached_prompt_tokens + excluded.cached_prompt_tokens, completion_tokens = completion_tokens + excluded.completion_tokens, last_prompt_tokens = CASE WHEN excluded.last_prompt_tokens > 0 THEN excluded.last_prompt_tokens ELSE last_prompt_tokens END, context_tokens = CASE WHEN excluded.context_tokens > 0 THEN excluded.context_tokens ELSE context_tokens END, model_ms = model_ms + excluded.model_ms")
            .bind(stored(usage.steps))
            .bind(stored(usage.prompt_tokens))
            .bind(stored(usage.cached_prompt_tokens))
            .bind(stored(usage.completion_tokens))
            .bind(stored(usage.last_prompt_tokens))
            .bind(stored(usage.context_tokens))
            .bind(stored(usage.model_ms))
            .bind(room_id)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// `None` when the room has no such AI; zeros before its first turn.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn agent_usage(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Option<AgentUsageTotals>, PersistenceError> {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE room_id = ? AND session_id = ?)",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_one(&self.pool)
        .await?;
        if !exists {
            return Ok(None);
        }
        let row = sqlx::query("SELECT * FROM agent_usage WHERE room_id = ? AND session_id = ?")
            .bind(room_id)
            .bind(session_id)
            .fetch_optional(&self.pool)
            .await?;
        Ok(Some(row.map_or_else(AgentUsageTotals::default, |row| {
            AgentUsageTotals {
                turns: loaded(row.get("turns")),
                steps: loaded(row.get("steps")),
                prompt_tokens: loaded(row.get("prompt_tokens")),
                cached_prompt_tokens: loaded(row.get("cached_prompt_tokens")),
                completion_tokens: loaded(row.get("completion_tokens")),
                last_prompt_tokens: loaded(row.get("last_prompt_tokens")),
                context_tokens: loaded(row.get("context_tokens")),
                model_ms: loaded(row.get("model_ms")),
            }
        })))
    }
}

pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 102) {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '102' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
