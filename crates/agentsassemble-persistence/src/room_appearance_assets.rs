use std::collections::BTreeSet;

use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, MAX_ATTACHMENT_BYTES,
    ROOM_APPEARANCE_ASSET_PREFIX, ROOM_APPEARANCE_REFERENCE_PREFIX,
    ROOM_APPEARANCE_REFERENCE_SUFFIX, RoomAppearance, is_room_appearance_asset_id,
    room_appearance_asset_id,
};
use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanSessionAuthorization, LocalRoomManagerAuthority, PersistenceError, SqliteStore,
    asset_storage::enforce_storage_replacement,
    human_session_authority::revalidate_human_session,
    raster_assets::{prepare_raster, sanitize_filename, validate_stored_raster},
    room_user_identity::{
        require_current_local_room_manager, require_exact_local_room_manager,
        resolve_room_user_identity,
    },
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
        enforce_storage_replacement(&mut transaction, None, size).await?;
        let asset_id = format!("{ROOM_APPEARANCE_ASSET_PREFIX}{}", Uuid::new_v4().simple());
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
            usize::try_from(size).unwrap_or(MAX_ATTACHMENT_BYTES),
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

    /// Reads one room-owned PNG only while a current human member can reach its exact reference.
    ///
    /// # Errors
    ///
    /// Fails closed for malformed IDs, stale membership, cross-room or unreferenced assets, and
    /// corrupt stored state.
    pub async fn bound_room_appearance_asset(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        asset_id: &str,
    ) -> Result<RoomAppearanceAsset, PersistenceError> {
        if !valid_asset_id(asset_id) {
            return Err(asset_missing());
        }
        let mut transaction = self.pool.begin().await?;
        resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        let asset = read_bound_room_appearance_asset(&mut transaction, room_id, asset_id).await?;
        transaction.commit().await?;
        Ok(asset)
    }

    /// Reads one room-owned PNG while retaining exact durable human-session provenance.
    ///
    /// # Errors
    ///
    /// Fails closed when the issued session authority has expired or changed, the human has left,
    /// or the requested asset is no longer an intact reference owned by that room.
    pub async fn bound_human_session_room_appearance_asset(
        &self,
        authorization: &HumanSessionAuthorization,
        asset_id: &str,
    ) -> Result<RoomAppearanceAsset, PersistenceError> {
        if !valid_asset_id(asset_id) {
            return Err(asset_missing());
        }
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        let asset = read_bound_room_appearance_asset(
            &mut transaction,
            &current.principal().room_id,
            asset_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(asset)
    }
}

async fn read_bound_room_appearance_asset(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    asset_id: &str,
) -> Result<RoomAppearanceAsset, PersistenceError> {
    let row = sqlx::query(
            "SELECT asset.filename, asset.content_type, asset.size, length(asset.content) AS content_length, asset.created_at, room.settings_json FROM room_appearance_assets AS asset INNER JOIN rooms AS room ON room.room_id = asset.room_id WHERE asset.asset_id = ? AND asset.room_id = ? AND asset.state = 'bound' AND asset.pending_owner_user_id IS NULL AND asset.expires_at IS NULL",
        )
        .bind(asset_id)
        .bind(room_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(asset_missing)?;
    let settings: agentsassemble_domain::RoomSettings =
        serde_json::from_str(row.get::<String, _>("settings_json").as_str())?;
    let referenced = [
        &settings.appearance.banner_image_url,
        &settings.appearance.icon_image_url,
    ]
    .into_iter()
    .filter_map(|url| room_appearance_asset_id(url))
    .any(|referenced_id| referenced_id == asset_id);
    if !referenced {
        return Err(asset_missing());
    }
    let (filename, size) = validate_asset_metadata(&row, row.get("content_length"))?;
    let content = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT content FROM room_appearance_assets WHERE asset_id = ? AND room_id = ? AND state = 'bound'",
        )
        .bind(asset_id)
        .bind(room_id)
        .fetch_one(&mut **transaction)
        .await?;
    if content.len() != size {
        return Err(invalid_asset_state());
    }
    let asset = RoomAppearanceAsset {
        metadata: asset_metadata(asset_id.to_owned(), filename, size),
        content,
    };
    Ok(asset)
}

fn decode_asset(
    asset_id: &str,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<RoomAppearanceAsset, PersistenceError> {
    let content = row.get::<Vec<u8>, _>("content");
    let (filename, size) =
        validate_asset_metadata(row, i64::try_from(content.len()).unwrap_or(i64::MAX))?;
    Ok(RoomAppearanceAsset {
        metadata: asset_metadata(asset_id.to_owned(), filename, size),
        content,
    })
}

fn validate_asset_metadata(
    row: &sqlx::sqlite::SqliteRow,
    content_length: i64,
) -> Result<(String, usize), PersistenceError> {
    let size = row.get::<i64, _>("size");
    validate_stored_raster(
        row.get::<String, _>("content_type").as_str(),
        size,
        content_length,
        row.get::<String, _>("created_at").as_str(),
    )?;
    Ok((
        sanitize_filename(row.get::<String, _>("filename").as_str()),
        usize::try_from(size).map_err(|_| invalid_asset_state())?,
    ))
}

fn asset_metadata(id: String, filename: String, size: usize) -> RoomAppearanceAssetMetadata {
    RoomAppearanceAssetMetadata {
        url: format!("{ROOM_APPEARANCE_REFERENCE_PREFIX}{id}{ROOM_APPEARANCE_REFERENCE_SUFFIX}"),
        id,
        filename,
        content_type: "image/png".to_owned(),
        size,
        is_image: true,
    }
}

fn valid_asset_id(asset_id: &str) -> bool {
    is_room_appearance_asset_id(asset_id)
}

pub(crate) async fn transition_room_appearance_references(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    current: &RoomAppearance,
    next: &RoomAppearance,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    let current_ids = appearance_asset_ids(current)?;
    let next_ids = appearance_asset_ids(next)?;
    if current.banner_image_url == next.banner_image_url
        && current.icon_image_url == next.icon_image_url
    {
        return Ok(());
    }

    let mut manager_user_id = None;
    for asset_id in &next_ids {
        let row = sqlx::query(
            "SELECT pending_owner_user_id, content_type, size, length(content) AS content_length, created_at, state, expires_at FROM room_appearance_assets WHERE asset_id = ? AND room_id = ?",
        )
        .bind(asset_id)
        .bind(room_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(asset_missing)?;
        validate_stored_raster(
            row.get::<String, _>("content_type").as_str(),
            row.get::<i64, _>("size"),
            row.get::<i64, _>("content_length"),
            row.get::<String, _>("created_at").as_str(),
        )?;
        let owner = row.get::<Option<String>, _>("pending_owner_user_id");
        let expires_at = row.get::<Option<i64>, _>("expires_at");
        match row.get::<String, _>("state").as_str() {
            "bound"
                if owner.is_none() && expires_at.is_none() && current_ids.contains(asset_id) => {}
            "pending" if !current_ids.contains(asset_id) => {
                let current_manager = if let Some(user_id) = &manager_user_id {
                    user_id
                } else {
                    let identity = resolve_room_user_identity(
                        transaction,
                        room_id,
                        LOCAL_OPERATOR_USER_ID,
                        LOCAL_OPERATOR_PARTICIPANT_ID,
                    )
                    .await?;
                    require_current_local_room_manager(transaction, &identity).await?;
                    manager_user_id.insert(identity.user_id)
                };
                if owner.as_deref() != Some(current_manager.as_str())
                    || expires_at.is_none_or(|expires_at| expires_at <= now.timestamp())
                {
                    return Err(asset_missing());
                }
                let promoted = sqlx::query(
                    "UPDATE room_appearance_assets SET state = 'bound', pending_owner_user_id = NULL, expires_at = NULL WHERE asset_id = ? AND room_id = ? AND pending_owner_user_id = ? AND state = 'pending' AND expires_at > ?",
                )
                .bind(asset_id)
                .bind(room_id)
                .bind(current_manager)
                .bind(now.timestamp())
                .execute(&mut **transaction)
                .await?;
                if promoted.rows_affected() != 1 {
                    return Err(asset_missing());
                }
            }
            _ => return Err(invalid_asset_state()),
        }
    }

    for asset_id in current_ids.difference(&next_ids) {
        let deleted = sqlx::query(
            "DELETE FROM room_appearance_assets WHERE asset_id = ? AND room_id = ? AND state = 'bound'",
        )
        .bind(asset_id)
        .bind(room_id)
        .execute(&mut **transaction)
        .await?;
        if deleted.rows_affected() != 1 {
            return Err(invalid_asset_state());
        }
    }
    Ok(())
}

fn appearance_asset_ids(appearance: &RoomAppearance) -> Result<BTreeSet<String>, PersistenceError> {
    [&appearance.banner_image_url, &appearance.icon_image_url]
        .into_iter()
        .filter(|url| !url.is_empty())
        .map(|url| {
            room_appearance_asset_id(url)
                .map(str::to_owned)
                .ok_or_else(invalid_asset_state)
        })
        .collect()
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
