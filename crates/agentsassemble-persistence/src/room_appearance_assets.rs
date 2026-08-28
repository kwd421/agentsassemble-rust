use agentsassemble_domain::room_appearance_asset_id;
use chrono::{Duration, Utc};
use serde::Serialize;
use sqlx::Row;
use uuid::Uuid;

use crate::{
    LocalRoomManagerAuthority, PersistenceError, SqliteStore,
    raster_assets::{
        MAX_RASTER_BYTES, enforce_storage_replacement, prepare_raster, sanitize_filename,
        validate_stored_raster,
    },
    room_user_identity::require_exact_local_room_manager,
};

const PENDING_APPEARANCE_TTL: Duration = Duration::minutes(15);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoomAppearanceAssetMetadata {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub is_image: bool,
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomAppearanceAsset {
    pub metadata: RoomAppearanceAssetMetadata,
    pub content: Vec<u8>,
}

impl SqliteStore {
    /// Stores one canonical PNG under expiring room-manager custody.
    ///
    /// # Errors
    ///
    /// Fails closed for stale manager authority, malformed image bytes, exhausted shared raster
    /// capacity, or invalid durable state.
    pub async fn store_pending_room_appearance_asset(
        &self,
        authority: &LocalRoomManagerAuthority,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<RoomAppearanceAssetMetadata, PersistenceError> {
        let (canonical, size) = prepare_raster(filename, content_type, content).await?;
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        let manager = require_exact_local_room_manager(&mut transaction, authority).await?;
        delete_expired_pending(&mut transaction, now.timestamp()).await?;
        enforce_storage_replacement(&mut transaction, None, size, now.timestamp()).await?;
        let asset_id = format!("ra_{}", Uuid::new_v4().simple());
        sqlx::query(
            "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, ?, ?, ?, 'image/png', ?, ?, ?, 'pending', ?)",
        )
        .bind(&asset_id)
        .bind(&manager.room_id)
        .bind(&manager.user_id)
        .bind(&canonical.filename)
        .bind(canonical.content)
        .bind(size)
        .bind(now.to_rfc3339())
        .bind((now + PENDING_APPEARANCE_TTL).timestamp())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(asset_metadata(
            asset_id,
            canonical.filename,
            usize::try_from(size).unwrap_or(MAX_RASTER_BYTES),
        ))
    }

    /// Reads one live pending PNG only for its exact current room manager.
    ///
    /// # Errors
    ///
    /// Fails closed for malformed IDs, stale or mismatched authority, expiry, missing rows, or
    /// corrupt stored raster metadata.
    pub async fn pending_room_appearance_asset(
        &self,
        authority: &LocalRoomManagerAuthority,
        asset_id: &str,
    ) -> Result<RoomAppearanceAsset, PersistenceError> {
        if !valid_asset_id(asset_id) {
            return Err(asset_missing());
        }
        let now = Utc::now();
        let mut transaction = self.pool.begin().await?;
        let manager = require_exact_local_room_manager(&mut transaction, authority).await?;
        delete_expired_pending(&mut transaction, now.timestamp()).await?;
        let row = sqlx::query(
            "SELECT filename, content_type, content, size, created_at FROM room_appearance_assets WHERE asset_id = ? AND room_id = ? AND pending_owner_user_id = ? AND state = 'pending' AND expires_at > ?",
        )
        .bind(asset_id)
        .bind(&manager.room_id)
        .bind(&manager.user_id)
        .bind(now.timestamp())
        .fetch_optional(&mut *transaction)
        .await?;
        let asset = row
            .as_ref()
            .map(|row| decode_asset(asset_id, row))
            .transpose()?;
        transaction.commit().await?;
        asset.ok_or_else(asset_missing)
    }
}

fn decode_asset(
    asset_id: &str,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<RoomAppearanceAsset, PersistenceError> {
    let content_type = row.get::<String, _>("content_type");
    let content = row.get::<Vec<u8>, _>("content");
    let size = row.get::<i64, _>("size");
    validate_stored_raster(
        &content_type,
        size,
        i64::try_from(content.len()).unwrap_or(i64::MAX),
        row.get::<String, _>("created_at").as_str(),
    )?;
    let size = usize::try_from(size).map_err(|_| invalid_asset_state())?;
    Ok(RoomAppearanceAsset {
        metadata: asset_metadata(
            asset_id.to_owned(),
            sanitize_filename(row.get::<String, _>("filename").as_str()),
            size,
        ),
        content,
    })
}

fn asset_metadata(id: String, filename: String, size: usize) -> RoomAppearanceAssetMetadata {
    RoomAppearanceAssetMetadata {
        url: format!("/api/attachments/{id}?view=1"),
        id,
        filename,
        content_type: "image/png".to_owned(),
        size,
        is_image: true,
    }
}

fn valid_asset_id(asset_id: &str) -> bool {
    room_appearance_asset_id(&format!("/api/attachments/{asset_id}?view=1")) == Some(asset_id)
}

async fn delete_expired_pending(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    now_timestamp: i64,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "DELETE FROM room_appearance_assets WHERE state = 'pending' AND expires_at IS NOT NULL AND expires_at <= ?",
    )
    .bind(now_timestamp)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn asset_missing() -> PersistenceError {
    rejected(
        "appearance_asset_missing",
        "Room appearance asset was not found.",
    )
}

fn invalid_asset_state() -> PersistenceError {
    rejected(
        "invalid_state",
        "Stored room appearance asset metadata is invalid.",
    )
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
