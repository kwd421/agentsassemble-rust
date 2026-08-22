use std::str::FromStr;

use agentsassemble_domain::{
    AuthenticatedPrincipal, MessageSend, Participant, Room, RoomEvent, RoomSettings,
    canonical_payload_hash, prepare_message_event,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, SqlitePool, Transaction, sqlite::SqliteConnectOptions};
use thiserror::Error;

const SCHEMA_OWNER: &str = "agentsassemble-rust-v1";

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("persistent authority belongs to {0}, not this runtime")]
    AuthorityConflict(String),
    #[error("room does not exist")]
    RoomMissing,
    #[error("participant does not exist")]
    ParticipantMissing,
    #[error("request id was reused with a different action or payload")]
    CommandConflict,
    #[error("command rejected: {code}: {message}")]
    CommandRejected { code: &'static str, message: String },
}

#[derive(Debug, Clone)]
pub struct RoomSnapshotData {
    pub room: Room,
    pub settings: RoomSettings,
    pub participants: Vec<Participant>,
    pub events: Vec<RoomEvent>,
    pub oldest_seq: i64,
    pub last_seq: i64,
    pub has_more_before: bool,
}

#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub result: Value,
    pub event: RoomEvent,
    pub deduplicated: bool,
}

#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

impl SqliteStore {
    /// Opens the `SQLite` authority and verifies its ownership marker.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the store cannot be owned safely.
    pub async fn open(database_url: &str) -> Result<Self, PersistenceError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePool::connect_with(options).await?;
        let store = Self { pool };
        store.initialize().await?;
        Ok(store)
    }

    /// Installs the current schema in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error if initialization cannot complete.
    pub async fn initialize(&self) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        for statement in schema_statements() {
            sqlx::query(*statement).execute(&mut *transaction).await?;
        }
        let owner = sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
        )
        .fetch_optional(&mut *transaction)
        .await?;
        match owner {
            Some(owner) if owner != SCHEMA_OWNER => {
                return Err(PersistenceError::AuthorityConflict(owner));
            }
            Some(_) => {}
            None => {
                sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('schema_owner', ?)")
                    .bind(SCHEMA_OWNER)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Inserts an explicit room fixture and its host participant if absent.
    ///
    /// # Errors
    ///
    /// Returns a persistence or serialization error; partial inserts roll back.
    pub async fn bootstrap_room(
        &self,
        room: &Room,
        settings: &RoomSettings,
        participant: &Participant,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)",
        )
        .bind(&room.room_id)
        .bind(serde_json::to_string(room)?)
        .bind(serde_json::to_string(settings)?)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
        )
        .bind(&participant.room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(participant)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Reports whether the authoritative room row exists.
    ///
    /// # Errors
    ///
    /// Returns a database error if the query cannot complete.
    pub async fn room_exists(&self, room_id: &str) -> Result<bool, PersistenceError> {
        let found = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(found == 1)
    }

    /// Loads one authoritative participant.
    ///
    /// # Errors
    ///
    /// Returns `ParticipantMissing` or the underlying persistence error.
    pub async fn participant(
        &self,
        room_id: &str,
        participant_id: &str,
    ) -> Result<Participant, PersistenceError> {
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
        )
        .bind(room_id)
        .bind(participant_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PersistenceError::ParticipantMissing)?;
        Ok(serde_json::from_str(&encoded)?)
    }

    /// Reads a durable room snapshot after the supplied event cursor.
    ///
    /// # Errors
    ///
    /// Returns `RoomMissing` or the underlying persistence/serialization error.
    pub async fn snapshot(
        &self,
        room_id: &str,
        resume_from_seq: i64,
        limit: i64,
    ) -> Result<RoomSnapshotData, PersistenceError> {
        let row = sqlx::query("SELECT room_json, settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(PersistenceError::RoomMissing)?;
        let room = serde_json::from_str(row.try_get("room_json")?)?;
        let settings = serde_json::from_str(row.try_get("settings_json")?)?;
        let participant_rows = sqlx::query(
            "SELECT participant_json FROM participants WHERE room_id = ? ORDER BY participant_id",
        )
        .bind(room_id)
        .fetch_all(&self.pool)
        .await?;
        let participants = participant_rows
            .into_iter()
            .map(|row| serde_json::from_str(row.get::<String, _>("participant_json").as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let last_seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(room_id)
        .fetch_one(&self.pool)
        .await?;
        let oldest_seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MIN(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(room_id)
        .fetch_one(&self.pool)
        .await?;
        let event_rows = sqlx::query(
            "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? ORDER BY seq LIMIT ?",
        )
        .bind(room_id)
        .bind(resume_from_seq.max(0))
        .bind(limit.max(1))
        .fetch_all(&self.pool)
        .await?;
        let events = event_rows
            .into_iter()
            .map(|row| serde_json::from_str(row.get::<String, _>("event_json").as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let returned_last = events
            .last()
            .map_or(resume_from_seq.max(0), |event: &RoomEvent| event.seq);
        Ok(RoomSnapshotData {
            room,
            settings,
            participants,
            events,
            oldest_seq,
            last_seq,
            has_more_before: returned_last < last_seq,
        })
    }

    /// Commits a message event and its durable idempotency result atomically.
    ///
    /// # Errors
    ///
    /// Returns a visible command rejection, conflict, or persistence failure.
    pub async fn execute_message(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        if action != "message.send" {
            return Err(PersistenceError::CommandRejected {
                code: "unsupported_action",
                message: format!("Unsupported room command: {action}"),
            });
        }
        let command = MessageSend::from_payload(payload).map_err(rejection)?;
        let participant_json = sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
        )
        .bind(&principal.room_id)
        .bind(&principal.participant_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PersistenceError::ParticipantMissing)?;
        let participant: Participant = serde_json::from_str(&participant_json)?;
        let sequence = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
        )
        .bind(&principal.room_id)
        .fetch_one(&mut *transaction)
        .await?;
        let event = prepare_message_event(principal, &participant, &command, sequence, Utc::now())
            .map_err(rejection)?;
        sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(&principal.room_id)
            .bind(sequence)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *transaction)
            .await?;
        let result = json!({"event": event, "event_seq": sequence});
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(action)
        .bind(payload_hash)
        .bind(serde_json::to_string(&result)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommandOutcome {
            result,
            event,
            deduplicated: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn pool(&self) -> &SqlitePool {
        &self.pool
    }
}

async fn existing_command(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
    action: &str,
    payload_hash: &str,
) -> Result<Option<CommandOutcome>, PersistenceError> {
    let row = sqlx::query(
        "SELECT action, payload_hash, result_json FROM command_results WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_action: String = row.try_get("action")?;
    let stored_hash: String = row.try_get("payload_hash")?;
    if stored_action != action || stored_hash != payload_hash {
        return Err(PersistenceError::CommandConflict);
    }
    let result: Value = serde_json::from_str(row.try_get::<String, _>("result_json")?.as_str())?;
    let event = serde_json::from_value(result.get("event").cloned().ok_or_else(|| {
        serde_json::Error::io(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stored command result has no event",
        ))
    })?)?;
    Ok(Some(CommandOutcome {
        result,
        event,
        deduplicated: true,
    }))
}

fn rejection(error: agentsassemble_domain::CommandRejection) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: error.code,
        message: error.message,
    }
}

fn schema_statements() -> &'static [&'static str] {
    &[
        "CREATE TABLE IF NOT EXISTS runtime_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS rooms (room_id TEXT PRIMARY KEY, room_json TEXT NOT NULL, settings_json TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS participants (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, participant_json TEXT NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS room_events (room_id TEXT NOT NULL, seq INTEGER NOT NULL CHECK(seq > 0), event_json TEXT NOT NULL, PRIMARY KEY(room_id, seq), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS command_results (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
    ]
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant,
        ParticipantStatus, Room, RoomSettings,
    };
    use chrono::Utc;
    use serde_json::json;

    use super::{PersistenceError, SqliteStore};

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let url = format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );
        let store = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open fixture: {error}"));
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: "host".to_owned(),
            display_name: "Host".to_owned(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        store
            .bootstrap_room(
                &room,
                &RoomSettings::defaults("General".to_owned()),
                &participant,
            )
            .await
            .unwrap_or_else(|error| panic!("bootstrap fixture: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "local-operator".to_owned(),
            participant_id: "host".to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal)
    }

    #[tokio::test]
    async fn retry_is_deduplicated_and_payload_reuse_conflicts() {
        let (store, principal) = fixture().await;
        let payload = json!({"content": "hello"});
        let first = store
            .execute_message(&principal, "request-1", "message.send", &payload)
            .await
            .unwrap_or_else(|error| panic!("first command: {error}"));
        let retry = store
            .execute_message(&principal, "request-1", "message.send", &payload)
            .await
            .unwrap_or_else(|error| panic!("retry command: {error}"));
        assert!(!first.deduplicated);
        assert!(retry.deduplicated);
        assert_eq!(first.event.seq, retry.event.seq);
        assert!(matches!(
            store
                .execute_message(
                    &principal,
                    "request-1",
                    "message.send",
                    &json!({"content": "changed"})
                )
                .await,
            Err(PersistenceError::CommandConflict)
        ));
    }

    #[tokio::test]
    async fn command_result_failure_rolls_back_event() {
        let (store, principal) = fixture().await;
        sqlx::query(
            "CREATE TRIGGER reject_command_result BEFORE INSERT ON command_results BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(store.pool())
        .await
        .unwrap_or_else(|error| panic!("install failure trigger: {error}"));
        assert!(
            store
                .execute_message(
                    &principal,
                    "request-fails",
                    "message.send",
                    &json!({"content": "must roll back"}),
                )
                .await
                .is_err()
        );
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("read snapshot: {error}"));
        assert!(snapshot.events.is_empty());
        assert_eq!(snapshot.last_seq, 0);
    }
}
