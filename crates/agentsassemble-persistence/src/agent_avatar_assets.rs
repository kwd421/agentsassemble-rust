use agentsassemble_domain::{
    AGENT_AVATAR_ID_PREFIX, agent_avatar_asset_id, agent_avatar_url, is_agent_avatar_asset_id,
};
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, RoomManagerAuthority, SqliteStore,
    agent_lifecycle::{load_participant, load_session},
    agent_profile::{ProfileTarget, load_profile_target},
    asset_storage::enforce_storage_replacement,
    raster_assets::{prepare_raster, sanitize_filename, validate_stored_raster},
};

const PENDING_TTL: Duration = Duration::minutes(15);

/// External AIs have no Agent Session, so their pictures are kept by room
/// participant, under the same image identifiers and rules.
pub(crate) const CONNECTOR_DDL: &str = concat!(
    "CREATE TABLE IF NOT EXISTS connector_avatar_assets (",
    "asset_id TEXT NOT NULL PRIMARY KEY CHECK(length(asset_id) = 35 AND substr(asset_id, 1, 3) = 'aa_' AND substr(asset_id, 4) NOT GLOB '*[^0-9a-f]*'), ",
    "room_id TEXT NOT NULL, participant_id TEXT NOT NULL, filename TEXT NOT NULL, ",
    "content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, ",
    "size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 10485760 AND length(content) = size), ",
    "created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'current')), expires_at INTEGER, ",
    "CHECK((state = 'pending' AND expires_at IS NOT NULL) OR (state = 'current' AND expires_at IS NULL)), ",
    "UNIQUE(room_id, participant_id, state), ",
    "FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE)",
);

pub(crate) const EXPIRY_INDEX_DDL: &str = "CREATE INDEX IF NOT EXISTS connector_avatar_assets_expiry_idx ON connector_avatar_assets(expires_at) WHERE state = 'pending'";

pub(crate) const TABLES: &[crate::schema::TableDefinition] = &[crate::schema::TableDefinition {
    name: "connector_avatar_assets",
    ddl: CONNECTOR_DDL,
    infrastructure: false,
}];

/// Whose picture a stored avatar is: an Agent Session's, or an external AI's.
#[derive(Clone, Copy)]
pub(crate) enum AvatarHolder<'a> {
    Session { room_id: &'a str, session_id: &'a str },
    Connector { room_id: &'a str, participant_id: &'a str },
}

impl<'a> AvatarHolder<'a> {
    pub(crate) fn of(target: &'a ProfileTarget) -> Self {
        match target {
            ProfileTarget::Session(session, _) => Self::Session {
                room_id: &session.public.room_id,
                session_id: &session.public.session_id,
            },
            ProfileTarget::Connector(participant) => Self::Connector {
                room_id: &participant.room_id,
                participant_id: &participant.participant_id,
            },
        }
    }

    /// Table and key column, both fixed names.
    const fn custody(self) -> (&'static str, &'static str) {
        match self {
            Self::Session { .. } => ("agent_avatar_assets", "session_id"),
            Self::Connector { .. } => ("connector_avatar_assets", "participant_id"),
        }
    }

    const fn room_id(self) -> &'a str {
        match self {
            Self::Session { room_id, .. } | Self::Connector { room_id, .. } => room_id,
        }
    }

    const fn key(self) -> &'a str {
        match self {
            Self::Session { session_id, .. } => session_id,
            Self::Connector { participant_id, .. } => participant_id,
        }
    }

    fn sql(self, statement: &str) -> sqlx::AssertSqlSafe<String> {
        let (table, key) = self.custody();
        sqlx::AssertSqlSafe(statement.replace("{table}", table).replace("{key}", key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentAvatarMetadata {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub is_image: bool,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAvatarAsset {
    pub metadata: AgentAvatarMetadata,
    pub content: Vec<u8>,
}

impl SqliteStore {
    /// Stores one expiring pending raster under exact room-manager and Agent Session custody.
    ///
    /// # Errors
    /// Rejects stale authority, absent sessions, invalid raster, exhausted storage or corrupt state.
    pub async fn store_agent_avatar(
        &self,
        authority: &RoomManagerAuthority,
        session_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<AgentAvatarMetadata, PersistenceError> {
        let (raster, size) = prepare_raster(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let manager = authority.resolve(&mut transaction).await?;
        let target = load_profile_target(&mut transaction, &manager.room_id, session_id).await?;
        let holder = AvatarHolder::of(&target);
        let now = Utc::now();
        for table in ["agent_avatar_assets", "connector_avatar_assets"] {
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "DELETE FROM {table} WHERE state = 'pending' AND expires_at <= ?"
            )))
            .bind(now.timestamp())
            .execute(&mut *transaction)
            .await?;
        }
        let previous = sqlx::query(holder.sql(
            "SELECT asset_id, size FROM {table} WHERE room_id = ? AND {key} = ? AND state = 'pending'",
        ))
        .bind(holder.room_id())
        .bind(holder.key())
        .fetch_optional(&mut *transaction)
        .await?;
        enforce_storage_replacement(
            &mut transaction,
            previous.as_ref().map(|row| row.get::<i64, _>("size")),
            size,
        )
        .await?;
        if let Some(previous) = previous {
            delete_exact_asset(&mut transaction, holder, previous.get("asset_id"), "pending")
                .await?;
        }
        let id = format!("{AGENT_AVATAR_ID_PREFIX}{}", Uuid::new_v4().simple());
        sqlx::query(holder.sql("INSERT INTO {table}(asset_id, room_id, {key}, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, ?, ?, ?, 'image/png', ?, ?, ?, 'pending', ?)"))
            .bind(&id).bind(holder.room_id()).bind(holder.key()).bind(&raster.filename)
            .bind(&raster.content).bind(size).bind(now.to_rfc3339())
            .bind((now + PENDING_TTL).timestamp()).execute(&mut *transaction).await?;
        let metadata = metadata(id, raster.filename, raster.content.len())?;
        if let RoomManagerAuthority::Operator(session) = authority {
            crate::RoomMutationAuthority::OperatorSession(session)
                .record_success(&mut transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Reads only the exact current Agent avatar by its opaque public image identifier.
    ///
    /// # Errors
    /// Pending/unknown identifiers are not found; inconsistent durable custody fails closed.
    pub async fn agent_avatar<T>(
        &self,
        asset_id: &str,
        admit: impl FnOnce(usize) -> Result<T, PersistenceError>,
    ) -> Result<(AgentAvatarAsset, T), PersistenceError> {
        if !is_agent_avatar_asset_id(asset_id) {
            return Err(missing());
        }
        let mut transaction = self.pool.begin().await?;
        let agent_row = sqlx::query("SELECT room_id, session_id, filename, content_type, length(content) AS content_length, size, created_at FROM agent_avatar_assets WHERE asset_id = ? AND state = 'current'")
            .bind(asset_id).fetch_optional(&mut *transaction).await?;
        let (row, table) = if let Some(row) = agent_row {
            let session =
                load_session(&mut transaction, row.get("room_id"), row.get("session_id")).await?;
            if agent_avatar_asset_id(&session.public.avatar_image_url) != Some(asset_id) {
                return Err(invalid_custody());
            }
            (row, "agent_avatar_assets")
        } else {
            let row = sqlx::query("SELECT room_id, participant_id, filename, content_type, length(content) AS content_length, size, created_at FROM connector_avatar_assets WHERE asset_id = ? AND state = 'current'")
                .bind(asset_id).fetch_optional(&mut *transaction).await?.ok_or_else(missing)?;
            let participant =
                load_participant(&mut transaction, row.get("room_id"), row.get("participant_id")).await?;
            if agent_avatar_asset_id(&participant.avatar_image_url) != Some(asset_id) {
                return Err(invalid_custody());
            }
            (row, "connector_avatar_assets")
        };
        validate_stored_raster(
            row.get("content_type"),
            row.get("size"),
            row.get("content_length"),
            row.get("created_at"),
        )?;
        let size = usize::try_from(row.get::<i64, _>("size")).map_err(|_| invalid_custody())?;
        let lease = admit(size)?;
        let content: Vec<u8> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT content FROM {table} WHERE asset_id = ?"
        )))
        .bind(asset_id)
                .fetch_one(&mut *transaction)
                .await?;
        let metadata = metadata(
            asset_id.to_owned(),
            sanitize_filename(row.get("filename")),
            content.len(),
        )?;
        transaction.commit().await?;
        Ok((AgentAvatarAsset { metadata, content }, lease))
    }
}

pub(crate) async fn replace_agent_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    holder: AvatarHolder<'_>,
    current_reference: &str,
    next_reference: &str,
) -> Result<(), PersistenceError> {
    let previous_id = if current_reference.is_empty() {
        None
    } else {
        Some(agent_avatar_asset_id(current_reference).ok_or_else(invalid_custody)?)
    };
    let next_id = if next_reference.is_empty() {
        None
    } else {
        Some(agent_avatar_asset_id(next_reference).ok_or_else(|| {
            rejected(
                "bad_request",
                "Avatar must be an owned Agent avatar reference.",
            )
        })?)
    };
    let current_id = sqlx::query_scalar::<_, String>(holder.sql(
        "SELECT asset_id FROM {table} WHERE room_id = ? AND {key} = ? AND state = 'current'",
    ))
    .bind(holder.room_id())
    .bind(holder.key())
    .fetch_optional(&mut **transaction)
    .await?;
    if current_id.as_deref() != previous_id {
        return Err(invalid_custody());
    }
    if previous_id == next_id {
        return Ok(());
    }
    if let Some(id) = previous_id {
        delete_exact_asset(transaction, holder, id, "current").await?;
    }
    if let Some(id) = next_id {
        let changed = sqlx::query(holder.sql("UPDATE {table} SET state = 'current', expires_at = NULL WHERE asset_id = ? AND room_id = ? AND {key} = ? AND state = 'pending' AND expires_at > ?"))
            .bind(id).bind(holder.room_id()).bind(holder.key()).bind(Utc::now().timestamp())
            .execute(&mut **transaction).await?;
        if changed.rows_affected() != 1 {
            return Err(missing());
        }
    }
    Ok(())
}

async fn delete_exact_asset(
    transaction: &mut Transaction<'_, Sqlite>,
    holder: AvatarHolder<'_>,
    id: &str,
    state: &str,
) -> Result<(), PersistenceError> {
    let deleted = sqlx::query(holder.sql(
        "DELETE FROM {table} WHERE asset_id = ? AND room_id = ? AND {key} = ? AND state = ?",
    ))
    .bind(id)
    .bind(holder.room_id())
    .bind(holder.key())
    .bind(state)
    .execute(&mut **transaction)
    .await?;
    if deleted.rows_affected() != 1 {
        return Err(invalid_custody());
    }
    Ok(())
}

fn metadata(
    id: String,
    filename: String,
    size: usize,
) -> Result<AgentAvatarMetadata, PersistenceError> {
    let url = agent_avatar_url(&id).ok_or_else(invalid_custody)?;
    Ok(AgentAvatarMetadata {
        id,
        filename,
        content_type: "image/png".into(),
        size,
        is_image: true,
        url,
    })
}

fn invalid_custody() -> PersistenceError {
    rejected(
        "agent_avatar_custody_invalid",
        "Stored Agent avatar custody is inconsistent.",
    )
}
fn missing() -> PersistenceError {
    rejected("attachment_not_found", "Agent avatar was not found.")
}
fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 99) {
        sqlx::query(CONNECTOR_DDL).execute(&mut *tx).await?;
        sqlx::query(EXPIRY_INDEX_DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '99' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
