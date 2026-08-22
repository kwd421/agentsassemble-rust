use std::{fs::File, io, path::PathBuf, sync::Arc};

use agentsassemble_domain::{
    AuthenticatedPrincipal, MessageSend, Participant, ParticipantStatus, Room, RoomEvent,
    RoomSettings, RoomStatus, SnapshotMode, canonical_payload_hash, prepare_message_event,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, SqlitePool, Transaction, sqlite::SqlitePoolOptions};
use thiserror::Error;

use crate::database_target::PreparedDatabase;

const SCHEMA_OWNER: &str = "agentsassemble-rust-v1";

#[derive(Debug, Error)]
pub enum PersistenceError {
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("stored JSON is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("persistent authority belongs to {0}, not this runtime")]
    AuthorityConflict(String),
    #[error("existing nonempty database has no explicit Rust authority marker")]
    UnownedDatabase,
    #[error("another process already owns the database writer lease: {0}")]
    WriterAlreadyActive(PathBuf),
    #[error("writer lease operation failed: {0}")]
    WriterLease(#[source] io::Error),
    #[error("unsafe database authority: {0}")]
    UnsafeDatabasePath(&'static str),
    #[error("database initialization is allowed only for a newly created empty authority")]
    InitializationNotAllowed,
    #[error("room does not exist")]
    RoomMissing,
    #[error("participant does not exist")]
    ParticipantMissing,
    #[error("request id was reused with a different action or payload")]
    CommandConflict,
    #[error("snapshot cursor is ahead of durable room history at {durable_last_seq}")]
    InvalidCursor { durable_last_seq: i64 },
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
    pub resume_gap: bool,
    pub snapshot_mode: SnapshotMode,
}

#[derive(Debug, Clone)]
pub struct CommandOutcome {
    pub result: Value,
    pub event: RoomEvent,
    pub deduplicated: bool,
}

#[derive(Clone)]
pub struct SqliteStore {
    pub(crate) pool: SqlitePool,
    _writer_lease: Option<Arc<File>>,
    created: bool,
}

impl SqliteStore {
    /// Opens the `SQLite` authority and verifies its ownership marker.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the store cannot be owned safely.
    pub async fn open(database_url: &str) -> Result<Self, PersistenceError> {
        let prepared = PreparedDatabase::from_url(database_url)?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(prepared.options.clone().create_if_missing(true))
            .await?;
        prepared.revalidate()?;
        let store = Self {
            pool,
            _writer_lease: prepared.writer_lease,
            created: prepared.created,
        };
        if store.created {
            store.initialize().await?;
        } else {
            store.verify_owner().await?;
        }
        Ok(store)
    }

    /// Installs the current schema in one transaction.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error if initialization cannot complete.
    async fn initialize(&self) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        for statement in crate::schema::statements() {
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

    async fn verify_owner(&self) -> Result<(), PersistenceError> {
        let metadata_table = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runtime_metadata'",
        )
        .fetch_one(&self.pool)
        .await?;
        if metadata_table != 1 {
            return Err(PersistenceError::UnownedDatabase);
        }
        let owner = sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
        )
        .fetch_optional(&self.pool)
        .await?;
        match owner {
            Some(owner) if owner == SCHEMA_OWNER => Ok(()),
            Some(owner) => Err(PersistenceError::AuthorityConflict(owner)),
            None => Err(PersistenceError::UnownedDatabase),
        }
    }

    #[must_use]
    pub const fn was_created(&self) -> bool {
        self.created
    }

    /// Inserts an explicit room fixture and its host participant if absent.
    ///
    /// # Errors
    ///
    /// Returns a persistence or serialization error; partial inserts roll back.
    pub async fn initialize_room(
        &self,
        room: &Room,
        settings: &RoomSettings,
        participant: &Participant,
    ) -> Result<(), PersistenceError> {
        if !self.created {
            return Err(PersistenceError::InitializationNotAllowed);
        }
        let mut transaction = self.pool.begin().await?;
        let existing_rows = sqlx::query_scalar::<_, i64>(
            "SELECT (SELECT COUNT(*) FROM rooms) + (SELECT COUNT(*) FROM participants)",
        )
        .fetch_one(&mut *transaction)
        .await?;
        if existing_rows != 0 {
            return Err(PersistenceError::InitializationNotAllowed);
        }
        sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)")
            .bind(&room.room_id)
            .bind(serde_json::to_string(room)?)
            .bind(serde_json::to_string(settings)?)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
        )
        .bind(&participant.room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(participant)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Verifies that a principal still maps to an active room membership.
    ///
    /// # Errors
    ///
    /// Returns a stable session rejection or an underlying persistence error.
    pub async fn authorize_session(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Loads a participant only when both membership and room are active.
    ///
    /// # Errors
    ///
    /// Returns a stable inactive-room/session rejection or persistence error.
    pub async fn active_participant(
        &self,
        room_id: &str,
        participant_id: &str,
    ) -> Result<Participant, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let participant =
            load_active_participant(&mut transaction, room_id, participant_id).await?;
        transaction.commit().await?;
        Ok(participant)
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
        self.snapshot_inner(room_id, None, resume_from_seq, limit)
            .await
    }

    /// Reads a durable snapshot only while the principal remains joined.
    ///
    /// # Errors
    ///
    /// Returns a stable session rejection, cursor error, or persistence error.
    pub async fn snapshot_for(
        &self,
        principal: &AuthenticatedPrincipal,
        resume_from_seq: i64,
        limit: i64,
    ) -> Result<RoomSnapshotData, PersistenceError> {
        self.snapshot_inner(&principal.room_id, Some(principal), resume_from_seq, limit)
            .await
    }

    async fn snapshot_inner(
        &self,
        room_id: &str,
        principal: Option<&AuthenticatedPrincipal>,
        resume_from_seq: i64,
        limit: i64,
    ) -> Result<RoomSnapshotData, PersistenceError> {
        if resume_from_seq < 0 {
            return Err(PersistenceError::InvalidCursor {
                durable_last_seq: 0,
            });
        }
        let limit = limit.max(1);
        let mut transaction = self.pool.begin().await?;
        if let Some(principal) = principal {
            authorize_session(&mut transaction, principal).await?;
        }
        let row = sqlx::query("SELECT room_json, settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&mut *transaction)
            .await?
            .ok_or(PersistenceError::RoomMissing)?;
        let room = serde_json::from_str(row.try_get("room_json")?)?;
        let settings = serde_json::from_str(row.try_get("settings_json")?)?;
        let participant_rows = sqlx::query(
            "SELECT participant_json FROM participants WHERE room_id = ? ORDER BY participant_id",
        )
        .bind(room_id)
        .fetch_all(&mut *transaction)
        .await?;
        let participants = participant_rows
            .into_iter()
            .map(|row| serde_json::from_str(row.get::<String, _>("participant_json").as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let durable_last_seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(room_id)
        .fetch_one(&mut *transaction)
        .await?;
        if resume_from_seq > durable_last_seq {
            return Err(PersistenceError::InvalidCursor { durable_last_seq });
        }
        let resume_gap = resume_from_seq > 0 && durable_last_seq - resume_from_seq > limit;
        let snapshot_mode = if resume_from_seq == 0 {
            SnapshotMode::Initial
        } else if resume_gap {
            SnapshotMode::Gap
        } else {
            SnapshotMode::Resume
        };
        let event_rows = if resume_from_seq == 0 || resume_gap {
            sqlx::query(
                "SELECT event_json FROM (SELECT seq, event_json FROM room_events WHERE room_id = ? ORDER BY seq DESC LIMIT ?) ORDER BY seq",
            )
            .bind(room_id)
            .bind(limit)
            .fetch_all(&mut *transaction)
            .await?
        } else {
            sqlx::query(
                "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? ORDER BY seq",
            )
            .bind(room_id)
            .bind(resume_from_seq)
            .fetch_all(&mut *transaction)
            .await?
        };
        let events = event_rows
            .into_iter()
            .map(|row| serde_json::from_str(row.get::<String, _>("event_json").as_str()))
            .collect::<Result<Vec<_>, _>>()?;
        let oldest_seq = events.first().map_or(0, |event: &RoomEvent| event.seq);
        let last_seq = events
            .last()
            .map_or(resume_from_seq, |event: &RoomEvent| event.seq);
        let has_more_before = oldest_seq > 1;
        transaction.commit().await?;
        Ok(RoomSnapshotData {
            room,
            settings,
            participants,
            events,
            oldest_seq,
            last_seq,
            has_more_before,
            resume_gap,
            snapshot_mode,
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
        let room_json =
            sqlx::query_scalar::<_, String>("SELECT room_json FROM rooms WHERE room_id = ?")
                .bind(&principal.room_id)
                .fetch_optional(&mut *transaction)
                .await?
                .ok_or(PersistenceError::RoomMissing)?;
        let room: Room = serde_json::from_str(&room_json)?;
        if room.status != RoomStatus::Active {
            return Err(PersistenceError::CommandRejected {
                code: "room_inactive",
                message: "Closed or archived rooms do not accept commands.".to_owned(),
            });
        }
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
}

async fn authorize_session(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
) -> Result<(), PersistenceError> {
    let participant =
        load_active_participant(transaction, &principal.room_id, &principal.participant_id).await?;
    if participant.room_id != principal.room_id
        || participant.participant_id != principal.participant_id
    {
        return Err(PersistenceError::CommandRejected {
            code: "session_revoked",
            message: "This room session has ended.".to_owned(),
        });
    }
    Ok(())
}

async fn load_active_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    let room_json =
        sqlx::query_scalar::<_, String>("SELECT room_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(PersistenceError::RoomMissing)?;
    let room: Room = serde_json::from_str(&room_json)?;
    if room.status != RoomStatus::Active {
        return Err(PersistenceError::CommandRejected {
            code: "room_inactive",
            message: "Closed or archived rooms do not accept active sessions.".to_owned(),
        });
    }
    let participant_json = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PersistenceError::ParticipantMissing)?;
    let participant: Participant = serde_json::from_str(&participant_json)?;
    if participant.status != ParticipantStatus::Joined {
        return Err(PersistenceError::CommandRejected {
            code: "session_revoked",
            message: "This room session has ended.".to_owned(),
        });
    }
    Ok(participant)
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant,
        ParticipantStatus, Room, RoomSettings, SnapshotMode,
    };
    use chrono::Utc;
    use serde_json::json;
    use sqlx::{SqlitePool, sqlite::SqliteConnectOptions};

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
            participant_id: "operator-local".to_owned(),
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
            .initialize_room(
                &room,
                &RoomSettings::defaults("General".to_owned()),
                &participant,
            )
            .await
            .unwrap_or_else(|error| panic!("bootstrap fixture: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: "operator-local".to_owned(),
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
        .execute(&store.pool)
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

    #[tokio::test]
    async fn existing_unowned_database_is_not_modified() {
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let path = directory.path().join("foreign.sqlite3");
        let url = format!("sqlite://{}", path.display());
        let options = SqliteConnectOptions::from_str(&url)
            .unwrap_or_else(|error| panic!("parse foreign URL: {error}"))
            .create_if_missing(true);
        let foreign = SqlitePool::connect_with(options)
            .await
            .unwrap_or_else(|error| panic!("open foreign database: {error}"));
        sqlx::query("CREATE TABLE foreign_data(value TEXT NOT NULL)")
            .execute(&foreign)
            .await
            .unwrap_or_else(|error| panic!("create foreign table: {error}"));
        foreign.close().await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
                .unwrap_or_else(|error| panic!("secure foreign fixture: {error}"));
        }

        assert!(matches!(
            SqliteStore::open(&url).await,
            Err(PersistenceError::UnownedDatabase)
        ));
        let read_only = SqlitePool::connect_with(
            SqliteConnectOptions::from_str(&url)
                .unwrap_or_else(|error| panic!("parse read-only URL: {error}"))
                .read_only(true),
        )
        .await
        .unwrap_or_else(|error| panic!("reopen foreign database: {error}"));
        let metadata_tables = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runtime_metadata'",
        )
        .fetch_one(&read_only)
        .await
        .unwrap_or_else(|error| panic!("inspect foreign schema: {error}"));
        assert_eq!(metadata_tables, 0);
    }

    #[tokio::test]
    async fn file_database_has_one_process_writer() {
        let directory = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create temporary directory: {error}"));
        let path = directory.path().join("owned.sqlite3");
        let url = format!("sqlite://{}", path.display());
        let first = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open first writer: {error}"));
        assert!(matches!(
            SqliteStore::open(&url).await,
            Err(PersistenceError::WriterAlreadyActive(_))
        ));
        drop(first);
        SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open writer after lease release: {error}"));
    }

    #[tokio::test]
    async fn snapshot_boundaries_match_the_browser_contract() {
        let (store, principal) = fixture().await;
        for sequence in 1..=205 {
            store
                .execute_message(
                    &principal,
                    &format!("snapshot-{sequence}"),
                    "message.send",
                    &json!({"content": format!("message {sequence}")}),
                )
                .await
                .unwrap_or_else(|error| panic!("append snapshot fixture {sequence}: {error}"));
        }
        let initial = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("initial snapshot: {error}"));
        assert_eq!(initial.snapshot_mode, SnapshotMode::Initial);
        assert_eq!((initial.oldest_seq, initial.last_seq), (6, 205));
        assert!(initial.has_more_before);

        let resume = store
            .snapshot("general", 5, 200)
            .await
            .unwrap_or_else(|error| panic!("resume snapshot: {error}"));
        assert_eq!(resume.snapshot_mode, SnapshotMode::Resume);
        assert_eq!((resume.oldest_seq, resume.last_seq), (6, 205));
        assert!(!resume.resume_gap);

        let gap = store
            .snapshot("general", 4, 200)
            .await
            .unwrap_or_else(|error| panic!("gap snapshot: {error}"));
        assert_eq!(gap.snapshot_mode, SnapshotMode::Gap);
        assert_eq!((gap.oldest_seq, gap.last_seq), (6, 205));
        assert!(gap.resume_gap);

        let current = store
            .snapshot("general", 205, 200)
            .await
            .unwrap_or_else(|error| panic!("current snapshot: {error}"));
        assert_eq!(current.snapshot_mode, SnapshotMode::Resume);
        assert!(current.events.is_empty());
        assert_eq!((current.oldest_seq, current.last_seq), (0, 205));
    }
}
