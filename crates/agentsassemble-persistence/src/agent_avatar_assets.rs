use agentsassemble_domain::{
    AgentSession, agent_avatar_asset_id, agent_avatar_url, is_agent_avatar_asset_id,
};
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    LocalRoomManagerAuthority, PersistenceError, SqliteStore,
    agent_lifecycle::load_session,
    agent_profile::load_profile_target,
    asset_storage::enforce_storage_replacement,
    raster_assets::{prepare_raster, sanitize_filename, validate_stored_raster},
    room_user_identity::require_exact_local_room_manager,
};

const PENDING_TTL: Duration = Duration::minutes(15);

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
        authority: &LocalRoomManagerAuthority,
        session_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<AgentAvatarMetadata, PersistenceError> {
        let (raster, size) = prepare_raster(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let manager = require_exact_local_room_manager(&mut transaction, authority).await?;
        let (session, _) =
            load_profile_target(&mut transaction, &manager.room_id, session_id).await?;
        let now = Utc::now();
        sqlx::query("DELETE FROM agent_avatar_assets WHERE state = 'pending' AND expires_at <= ?")
            .bind(now.timestamp())
            .execute(&mut *transaction)
            .await?;
        let previous = sqlx::query("SELECT asset_id, size FROM agent_avatar_assets WHERE room_id = ? AND session_id = ? AND state = 'pending'")
            .bind(&manager.room_id).bind(&session.public.session_id)
            .fetch_optional(&mut *transaction).await?;
        enforce_storage_replacement(
            &mut transaction,
            previous.as_ref().map(|row| row.get::<i64, _>("size")),
            size,
        )
        .await?;
        if let Some(previous) = previous {
            delete_exact_asset(
                &mut transaction,
                &session.public,
                previous.get("asset_id"),
                "pending",
            )
            .await?;
        }
        let id = format!("aa_{}", Uuid::new_v4().simple());
        sqlx::query("INSERT INTO agent_avatar_assets(asset_id, room_id, session_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, ?, ?, ?, 'image/png', ?, ?, ?, 'pending', ?)")
            .bind(&id).bind(&manager.room_id).bind(&session.public.session_id).bind(&raster.filename)
            .bind(&raster.content).bind(size).bind(now.to_rfc3339())
            .bind((now + PENDING_TTL).timestamp()).execute(&mut *transaction).await?;
        let metadata = metadata(id, raster.filename, raster.content.len())?;
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Reads only the exact current Agent avatar by its opaque public image identifier.
    ///
    /// # Errors
    /// Pending/unknown identifiers are not found; inconsistent durable custody fails closed.
    pub async fn agent_avatar(&self, asset_id: &str) -> Result<AgentAvatarAsset, PersistenceError> {
        if !is_agent_avatar_asset_id(asset_id) {
            return Err(missing());
        }
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT room_id, session_id, filename, content_type, content, size, created_at FROM agent_avatar_assets WHERE asset_id = ? AND state = 'current'")
            .bind(asset_id).fetch_optional(&mut *transaction).await?.ok_or_else(missing)?;
        let session =
            load_session(&mut transaction, row.get("room_id"), row.get("session_id")).await?;
        if agent_avatar_asset_id(&session.public.avatar_image_url) != Some(asset_id) {
            return Err(invalid_custody());
        }
        let content = row.get::<Vec<u8>, _>("content");
        validate_stored_raster(
            row.get("content_type"),
            row.get("size"),
            i64::try_from(content.len()).map_err(|_| invalid_custody())?,
            row.get("created_at"),
        )?;
        let metadata = metadata(
            asset_id.to_owned(),
            sanitize_filename(row.get("filename")),
            content.len(),
        )?;
        transaction.commit().await?;
        Ok(AgentAvatarAsset { metadata, content })
    }
}

pub(crate) async fn replace_agent_avatar(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &AgentSession,
    next_reference: &str,
) -> Result<(), PersistenceError> {
    let previous_id = if session.avatar_image_url.is_empty() {
        None
    } else {
        Some(agent_avatar_asset_id(&session.avatar_image_url).ok_or_else(invalid_custody)?)
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
    let current_id = sqlx::query_scalar::<_, String>("SELECT asset_id FROM agent_avatar_assets WHERE room_id = ? AND session_id = ? AND state = 'current'")
        .bind(&session.room_id).bind(&session.session_id).fetch_optional(&mut **transaction).await?;
    if current_id.as_deref() != previous_id {
        return Err(invalid_custody());
    }
    if previous_id == next_id {
        return Ok(());
    }
    if let Some(id) = previous_id {
        delete_exact_asset(transaction, session, id, "current").await?;
    }
    if let Some(id) = next_id {
        let changed = sqlx::query("UPDATE agent_avatar_assets SET state = 'current', expires_at = NULL WHERE asset_id = ? AND room_id = ? AND session_id = ? AND state = 'pending' AND expires_at > ?")
            .bind(id).bind(&session.room_id).bind(&session.session_id).bind(Utc::now().timestamp())
            .execute(&mut **transaction).await?;
        if changed.rows_affected() != 1 {
            return Err(missing());
        }
    }
    Ok(())
}

async fn delete_exact_asset(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &AgentSession,
    id: &str,
    state: &str,
) -> Result<(), PersistenceError> {
    let deleted = sqlx::query("DELETE FROM agent_avatar_assets WHERE asset_id = ? AND room_id = ? AND session_id = ? AND state = ?")
        .bind(id).bind(&session.room_id).bind(&session.session_id).bind(state)
        .execute(&mut **transaction).await?;
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
        code,
        message: message.to_owned(),
    }
}
