//! Risu-style memory for API agents (after `RisuAI`'s `HypaMemory`, no code from it).
//!
//! An API agent sees only its last 50 room messages each turn. Older messages
//! are folded, in batches, into memory pieces that the agent's own model
//! summarizes at the start of a turn. Each turn then carries the pinned
//! messages and the pieces most related to the recent conversation. A piece
//! goes away with any message it summarized, and its owner can delete it.
use std::collections::BTreeSet;

use agentsassemble_domain::{DurableAgentSession, RoomEvent};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError,
    message_attachments::{message_has_visible_payload, message_visible_text},
};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS agent_memory_pieces (room_id TEXT NOT NULL, session_id TEXT NOT NULL, from_seq INTEGER NOT NULL CHECK(from_seq > 0), to_seq INTEGER NOT NULL CHECK(to_seq >= from_seq), summary TEXT NOT NULL CHECK(length(summary) BETWEEN 1 AND 12000), created_at INTEGER NOT NULL, hidden INTEGER NOT NULL DEFAULT 0 CHECK(hidden IN (0, 1)), PRIMARY KEY(room_id, session_id, from_seq), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE) STRICT";

pub(crate) const TABLES: &[crate::schema::TableDefinition] = &[crate::schema::TableDefinition {
    name: "agent_memory_pieces",
    ddl: DDL,
    infrastructure: false,
}];

/// Dropped messages wait until a batch this large is worth a summary call.
const FOLD_MIN_MESSAGES: i64 = 10;
const FOLD_MAX_MESSAGES: i64 = 60;
const FOLD_MAX_CHARACTERS: usize = 20_000;
const LINE_MAX_CHARACTERS: usize = 600;
const MEMORY_MAX_CHARACTERS: usize = 6_000;
const PINNED_MAX_CHARACTERS: usize = 3_000;
/// A stored piece always fits the memory budget beside others.
const PIECE_MAX_CHARACTERS: usize = 3_000;
/// Only the newest pieces are weighed each turn.
const MEMORY_MAX_PIECES: i64 = 200;

/// What one turn carries: memory to show and, when due, a batch to summarize.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TurnMemoryInput {
    /// Pinned messages and related memory pieces, ready to show the model.
    pub context: String,
    /// Older messages no piece covers yet, summarized by the harness this turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fold: Option<MemoryFold>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct MemoryFold {
    pub from_seq: i64,
    pub to_seq: i64,
    pub text: String,
}

/// Only API agents on the built-in harness keep memory: the others hold their
/// own provider session history.
pub(crate) fn keeps_memory(session: &DurableAgentSession) -> bool {
    session.public.runtime_kind == "api" && session.public.execution_harness == "builtin"
}

pub(crate) async fn prepare(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session: &DurableAgentSession,
    oldest_shown_seq: i64,
    recent_text: &str,
) -> Result<Option<TurnMemoryInput>, PersistenceError> {
    if !keeps_memory(session) {
        return Ok(None);
    }
    let session_id = &session.public.session_id;
    let covered: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(to_seq), 0) FROM agent_memory_pieces WHERE room_id = ? AND session_id = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_one(&mut **tx)
    .await?;
    let floor = covered.max(session.public.bootstrap_cutoff_seq);
    let fold = load_fold(tx, room_id, floor, oldest_shown_seq).await?;
    let mut sections = Vec::new();
    let pinned = load_pinned(tx, room_id, session.public.bootstrap_cutoff_seq).await?;
    if !pinned.is_empty() {
        sections.push(format!("Pinned messages (important):\n{pinned}"));
    }
    let pieces = related_pieces(tx, room_id, session_id, recent_text).await?;
    if !pieces.is_empty() {
        sections.push(format!(
            "Memory of earlier conversation you no longer see in full:\n{pieces}"
        ));
    }
    if sections.is_empty() && fold.is_none() {
        return Ok(None);
    }
    Ok(Some(TurnMemoryInput {
        context: sections.join("\n\n"),
        fold,
    }))
}

/// The fold text of exactly `from_seq..=to_seq`, as `load_fold` renders it.
async fn render_range(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    from_seq: i64,
    to_seq: i64,
) -> Result<String, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq >= ? AND seq <= ? AND json_extract(event_json, '$.type') = 'message_final' AND COALESCE(json_extract(event_json, '$.message_deleted'), 0) = 0 AND COALESCE(json_extract(event_json, '$.message_kind'), '') NOT IN ('vote_cast', 'vote_withdraw', 'vote_close') ORDER BY seq",
    )
    .bind(room_id)
    .bind(from_seq)
    .bind(to_seq)
    .fetch_all(&mut **tx)
    .await?;
    let mut lines = Vec::new();
    for row in rows {
        let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
        if message_has_visible_payload(&event)? {
            lines.push(render_line(&event)?);
        }
    }
    Ok(lines.join("\n"))
}

async fn load_fold(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    floor: i64,
    oldest_shown_seq: i64,
) -> Result<Option<MemoryFold>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? AND seq < ? AND json_extract(event_json, '$.type') = 'message_final' AND COALESCE(json_extract(event_json, '$.message_deleted'), 0) = 0 AND COALESCE(json_extract(event_json, '$.message_kind'), '') NOT IN ('vote_cast', 'vote_withdraw', 'vote_close') ORDER BY seq LIMIT ?",
    )
    .bind(room_id)
    .bind(floor)
    .bind(oldest_shown_seq)
    .bind(FOLD_MAX_MESSAGES)
    .fetch_all(&mut **tx)
    .await?;
    if i64::try_from(rows.len()).unwrap_or(0) < FOLD_MIN_MESSAGES {
        return Ok(None);
    }
    let mut lines = Vec::new();
    let mut used = 0;
    let (mut from_seq, mut to_seq) = (0, 0);
    for row in rows {
        let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
        if !message_has_visible_payload(&event)? {
            continue;
        }
        let line = render_line(&event)?;
        if used + line.chars().count() > FOLD_MAX_CHARACTERS && !lines.is_empty() {
            break;
        }
        used += line.chars().count();
        if from_seq == 0 {
            from_seq = event.seq;
        }
        to_seq = event.seq;
        lines.push(line);
    }
    if i64::try_from(lines.len()).unwrap_or(0) < FOLD_MIN_MESSAGES {
        return Ok(None);
    }
    Ok(Some(MemoryFold {
        from_seq,
        to_seq,
        text: lines.join("\n"),
    }))
}

fn render_line(event: &RoomEvent) -> Result<String, PersistenceError> {
    let name = event
        .display_name
        .as_deref()
        .unwrap_or(&event.actor.participant_id)
        .replace(['\r', '\n'], " ");
    let text: String = message_visible_text(event)?
        .replace(['\r', '\n'], " ")
        .chars()
        .take(LINE_MAX_CHARACTERS)
        .collect();
    Ok(format!("#{} {name}: {text}", event.seq))
}

/// Pins from after the agent joined, the same horizon as its message window.
async fn load_pinned(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    joined_after_seq: i64,
) -> Result<String, PersistenceError> {
    let rows = sqlx::query(
        "SELECT e.event_json FROM room_message_pins p JOIN room_events e ON e.room_id = p.room_id AND e.seq = p.event_seq WHERE p.room_id = ? AND p.event_seq > ? AND json_extract(e.event_json, '$.type') = 'message_final' ORDER BY p.event_seq",
    )
    .bind(room_id)
    .bind(joined_after_seq)
    .fetch_all(&mut **tx)
    .await?;
    let mut lines = Vec::new();
    let mut used = 0;
    for row in rows {
        let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
        if !event.is_current_lobby_message() || !message_has_visible_payload(&event)? {
            continue;
        }
        let line = format!("- {}", render_line(&event)?);
        if used + line.chars().count() > PINNED_MAX_CHARACTERS {
            continue;
        }
        used += line.chars().count();
        lines.push(line);
    }
    Ok(lines.join("\n"))
}

/// Pieces sharing the most words with the recent conversation, shown in the
/// order they happened, within the memory budget.
async fn related_pieces(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    recent_text: &str,
) -> Result<String, PersistenceError> {
    let rows = sqlx::query(
        "SELECT from_seq, to_seq, summary FROM agent_memory_pieces WHERE room_id = ? AND session_id = ? AND hidden = 0 ORDER BY from_seq DESC LIMIT ?",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(MEMORY_MAX_PIECES)
    .fetch_all(&mut **tx)
    .await?;
    // The newest pieces were read; rank them in the order they happened.
    let rows: Vec<_> = rows.into_iter().rev().collect();
    let recent = words(recent_text);
    let mut scored: Vec<(usize, usize, String)> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let summary: String = row.get("summary");
            let text = format!(
                "- (#{}–#{}) {summary}",
                row.get::<i64, _>("from_seq"),
                row.get::<i64, _>("to_seq")
            );
            (words(&summary).intersection(&recent).count(), index, text)
        })
        .collect();
    // Most related first; among equals, the newer piece.
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut chosen = Vec::new();
    let mut used = 0;
    for (_, index, text) in scored {
        let size = text.chars().count();
        if used + size > MEMORY_MAX_CHARACTERS {
            continue;
        }
        used += size;
        chosen.push((index, text));
    }
    chosen.sort_by_key(|(index, _)| *index);
    Ok(chosen
        .into_iter()
        .map(|(_, text)| text)
        .collect::<Vec<_>>()
        .join("\n"))
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|character: char| !character.is_alphanumeric())
        .filter(|word| word.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect()
}

/// Stores the summary of a fold the harness completed. The fold must still
/// read exactly as it did when summarized: a message deleted or edited
/// meanwhile could survive in the summary, so that summary is dropped (and
/// the range is folded again). A fold another piece covered is dropped too.
pub(crate) async fn store(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    fold: (i64, i64),
    folded_text: &str,
    summary: &str,
    now: i64,
) -> Result<bool, PersistenceError> {
    let summary: String = summary.trim().chars().take(PIECE_MAX_CHARACTERS).collect();
    let (from_seq, to_seq) = fold;
    if summary.is_empty() || from_seq <= 0 || to_seq < from_seq {
        return Ok(false);
    }
    if render_range(tx, room_id, from_seq, to_seq).await? != folded_text {
        return Ok(false);
    }
    let inserted = sqlx::query(
        "INSERT INTO agent_memory_pieces(room_id, session_id, from_seq, to_seq, summary, created_at) \
         SELECT ?, ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM agent_memory_pieces \
         WHERE room_id = ? AND session_id = ? AND to_seq >= ?) \
         AND EXISTS (SELECT 1 FROM agent_sessions WHERE room_id = ? AND session_id = ?)",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(from_seq)
    .bind(to_seq)
    .bind(&summary)
    .bind(now)
    .bind(room_id)
    .bind(session_id)
    .bind(from_seq)
    .bind(room_id)
    .bind(session_id)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    Ok(inserted == 1)
}

/// One memory piece as its owner sees it in the memory tab.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AgentMemoryPiece {
    pub from_seq: i64,
    pub to_seq: i64,
    pub summary: String,
    pub created_at: i64,
}

impl crate::SqliteStore {
    /// Stores the summary the harness wrote for this turn's fold. A fold that
    /// another piece covered meanwhile is dropped (returns false).
    ///
    /// # Errors
    /// Storage failures.
    pub async fn store_agent_memory(
        &self,
        room_id: &str,
        session_id: &str,
        fold: (i64, i64),
        folded_text: &str,
        summary: &str,
    ) -> Result<bool, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let stored = store(
            &mut tx,
            room_id,
            session_id,
            fold,
            folded_text,
            summary,
            chrono::Utc::now().timestamp(),
        )
        .await?;
        tx.commit().await?;
        Ok(stored)
    }

    /// The agent's memory pieces, oldest first.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn agent_memory(
        &self,
        room_id: &str,
        session_id: &str,
    ) -> Result<Vec<AgentMemoryPiece>, PersistenceError> {
        let rows = sqlx::query(
            "SELECT from_seq, to_seq, summary, created_at FROM agent_memory_pieces WHERE room_id = ? AND session_id = ? AND hidden = 0 ORDER BY from_seq",
        )
        .bind(room_id)
        .bind(session_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|row| AgentMemoryPiece {
                from_seq: row.get("from_seq"),
                to_seq: row.get("to_seq"),
                summary: row.get("summary"),
                created_at: row.get("created_at"),
            })
            .collect())
    }

    /// Deletes one piece's memory. The row stays, emptied and hidden, so its
    /// messages are not folded into memory again.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn delete_agent_memory(
        &self,
        room_id: &str,
        session_id: &str,
        from_seq: i64,
    ) -> Result<bool, PersistenceError> {
        Ok(sqlx::query(
            "UPDATE agent_memory_pieces SET summary = '-', hidden = 1 WHERE room_id = ? AND session_id = ? AND from_seq = ? AND hidden = 0",
        )
        .bind(room_id)
        .bind(session_id)
        .bind(from_seq)
        .execute(&self.pool)
        .await?
        .rows_affected()
            == 1)
    }
}

/// A removed agent takes its memory with it.
pub(crate) async fn forget_session(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<(), PersistenceError> {
    sqlx::query("DELETE FROM agent_memory_pieces WHERE room_id = ? AND session_id = ?")
        .bind(room_id)
        .bind(session_id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

/// A deleted or edited message takes every piece that summarized it, except
/// pieces the owner already deleted (their range stays out of memory).
pub(crate) async fn forget_message(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    seq: i64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "DELETE FROM agent_memory_pieces WHERE room_id = ? AND from_seq <= ? AND to_seq >= ? AND hidden = 0",
    )
    .bind(room_id)
    .bind(seq)
    .bind(seq)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 98) {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '98' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "agent_memory_tests.rs"]
mod tests;
