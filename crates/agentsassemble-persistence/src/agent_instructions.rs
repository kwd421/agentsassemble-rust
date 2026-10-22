//! The owner's own settings for one AI in a room, for app AIs and external AIs
//! alike: its instructions, and whether it wakes only when mentioned. They are private to the owner: kept apart from the public
//! participant record and read only on the owner routes, by the AI's prompt and
//! by that external AI's own connector.
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS agent_instructions (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, instructions TEXT NOT NULL CHECK(length(instructions) <= 4000), updated_at INTEGER NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE) STRICT";

/// An AI listed here wakes only when a message mentions it, whatever the room's
/// conversation mode.
pub(crate) const MENTION_ONLY_DDL: &str = "CREATE TABLE IF NOT EXISTS agent_mention_only (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE) STRICT";

pub(crate) const TABLES: &[crate::schema::TableDefinition] = &[
    crate::schema::TableDefinition {
        name: "agent_instructions",
        ddl: DDL,
        infrastructure: false,
    },
    crate::schema::TableDefinition {
        name: "agent_mention_only",
        ddl: MENTION_ONLY_DDL,
        infrastructure: false,
    },
];

/// Characters an owner may give one AI.
pub const MAX_AGENT_INSTRUCTIONS_CHARS: usize = 4000;

/// The current instructions and when they last changed (microseconds).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentInstructions {
    pub instructions: String,
    pub updated_at: i64,
    /// Wakes only when a message mentions it.
    pub mention_only: bool,
}

impl SqliteStore {
    /// `None` when the room has no such AI; empty instructions when none are set.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn agent_instructions(
        &self,
        room_id: &str,
        participant_id: &str,
    ) -> Result<Option<AgentInstructions>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        if !is_agent(&mut tx, room_id, participant_id).await? {
            return Ok(None);
        }
        let current = instructions_in(&mut tx, room_id, participant_id).await?;
        let mention_only = mention_only_in(&mut tx, room_id, participant_id).await?;
        tx.commit().await?;
        Ok(Some(current.map_or(
            AgentInstructions {
                instructions: String::new(),
                updated_at: 0,
                mention_only,
            },
            |current| AgentInstructions {
                mention_only,
                ..current
            },
        )))
    }

    /// Makes an AI wake only when mentioned, or follow the room again. Returns
    /// false when the room has no such AI.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn set_agent_mention_only(
        &self,
        room_id: &str,
        participant_id: &str,
        mention_only: bool,
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        if !is_agent(&mut tx, room_id, participant_id).await? {
            return Ok(false);
        }
        let statement = if mention_only {
            "INSERT OR IGNORE INTO agent_mention_only(room_id, participant_id) VALUES(?, ?)"
        } else {
            "DELETE FROM agent_mention_only WHERE room_id = ? AND participant_id = ?"
        };
        sqlx::query(statement)
            .bind(room_id)
            .bind(participant_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Replaces the instructions; blank text removes them. Returns false when
    /// the room has no such AI, or it was removed from the room.
    ///
    /// # Errors
    /// Rejects text over the limit, and storage failures.
    pub async fn set_agent_instructions(
        &self,
        room_id: &str,
        participant_id: &str,
        instructions: &str,
        now: DateTime<Utc>,
    ) -> Result<bool, PersistenceError> {
        let instructions = instructions.trim();
        if instructions.chars().count() > MAX_AGENT_INSTRUCTIONS_CHARS {
            return Err(PersistenceError::CommandRejected {
                code: "bad_request".into(),
                message: format!(
                    "Instructions are limited to {MAX_AGENT_INSTRUCTIONS_CHARS} characters."
                ),
            });
        }
        let mut tx = self.pool.begin().await?;
        if !is_agent(&mut tx, room_id, participant_id).await? {
            return Ok(false);
        }
        // Blank text stays as an empty row, so the change keeps a revision an
        // external AI can be told about. The revision only ever grows.
        sqlx::query("INSERT INTO agent_instructions(room_id, participant_id, instructions, updated_at) VALUES(?, ?, ?, ?) ON CONFLICT(room_id, participant_id) DO UPDATE SET instructions = excluded.instructions, updated_at = MAX(excluded.updated_at, agent_instructions.updated_at + 1)")
            .bind(room_id)
            .bind(participant_id)
            .bind(instructions)
            .bind(now.timestamp_micros())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(true)
    }
}

async fn is_agent(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM participants WHERE room_id = ? AND participant_id = ? AND json_extract(participant_json, '$.participant_type') = 'agent' AND coalesce(json_extract(participant_json, '$.status'), '') NOT IN ('kicked', 'exported')",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_one(&mut **tx)
    .await?
        == 1)
}

pub(crate) async fn instructions_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Option<AgentInstructions>, PersistenceError> {
    Ok(sqlx::query(
        "SELECT instructions, updated_at FROM agent_instructions WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **tx)
    .await?
    .map(|row| AgentInstructions {
        instructions: row.get("instructions"),
        updated_at: row.get("updated_at"),
        mention_only: false,
    }))
}

/// A removed AI takes its owner settings with it.
pub(crate) async fn forget(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    for statement in [
        "DELETE FROM agent_instructions WHERE room_id = ? AND participant_id = ?",
        "DELETE FROM agent_mention_only WHERE room_id = ? AND participant_id = ?",
    ] {
        sqlx::query(statement)
            .bind(room_id)
            .bind(participant_id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

pub(crate) async fn mention_only_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM agent_mention_only WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_one(&mut **tx)
    .await?
        == 1)
}

pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 101) {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query(MENTION_ONLY_DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '101' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
