use agentsassemble_domain::{
    AttachmentStorageCount, AttachmentStorageUsage, MAX_ROOM_FILE_BYTES, RoomAttachmentStorageUsage,
};
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore};

pub(crate) const MAX_RETAINED_ASSETS: i64 = 4096;
const MAX_RETAINED_ASSET_BYTES: i64 = 8 * 1024 * 1024 * 1024;

pub(crate) async fn enforce_storage_replacement(
    transaction: &mut Transaction<'_, Sqlite>,
    previous_size: Option<i64>,
    new_size: i64,
) -> Result<(), PersistenceError> {
    if !(1..=i64::try_from(MAX_ROOM_FILE_BYTES).unwrap_or(i64::MAX)).contains(&new_size)
        || previous_size.is_some_and(|size| size <= 0)
    {
        return Err(invalid_storage_usage());
    }
    let row = sqlx::query(
        "SELECT COUNT(*) AS asset_count, COALESCE(SUM(size), 0) AS asset_bytes FROM (SELECT size FROM profile_avatar_assets UNION ALL SELECT size FROM prejoin_avatar_assets UNION ALL SELECT size FROM room_appearance_assets UNION ALL SELECT size FROM room_message_attachments UNION ALL SELECT size FROM agent_avatar_assets UNION ALL SELECT size FROM room_connector_uploads)",
    )
    .fetch_one(&mut **transaction)
    .await?;
    let current_count = row.try_get::<i64, _>("asset_count")?;
    let current_bytes = row.try_get::<i64, _>("asset_bytes")?;
    let previous_count = i64::from(previous_size.is_some());
    let next_count = current_count
        .checked_sub(previous_count)
        .and_then(|count| count.checked_add(1))
        .ok_or_else(invalid_storage_usage)?;
    let next_bytes = current_bytes
        .checked_sub(previous_size.unwrap_or(0))
        .and_then(|bytes| bytes.checked_add(new_size))
        .ok_or_else(invalid_storage_usage)?;
    if next_count > MAX_RETAINED_ASSETS || next_bytes > MAX_RETAINED_ASSET_BYTES {
        return Err(rejected(
            "attachment_quota_reached",
            "Absolute attachment storage limit reached.",
        ));
    }
    Ok(())
}

fn invalid_storage_usage() -> PersistenceError {
    rejected("invalid_state", "Stored attachment usage is invalid.")
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

impl SqliteStore {
    /// One statement gives room labels and retained sizes from the same read snapshot.
    /// Deliberately includes expired pending rows until their storage owner deletes them.
    pub async fn attachment_storage_usage(
        &self,
    ) -> Result<AttachmentStorageUsage, PersistenceError> {
        let rows = sqlx::query(
            "WITH assets AS (
                SELECT room_id, size FROM room_message_attachments
                UNION ALL SELECT room_id, size FROM room_connector_uploads
                UNION ALL SELECT room_id, size FROM room_appearance_assets
                UNION ALL SELECT room_id, size FROM agent_avatar_assets
                UNION ALL SELECT room_id, size FROM prejoin_avatar_assets
                UNION ALL SELECT NULL, size FROM profile_avatar_assets
            ), usage AS (
                SELECT room_id, COUNT(*) AS files, SUM(size) AS bytes
                FROM assets GROUP BY room_id
            )
            SELECT rooms.room_id AS room_id, json_extract(rooms.settings_json, '$.label') AS label,
                COALESCE(usage.files, 0) AS files, COALESCE(usage.bytes, 0) AS bytes
            FROM rooms LEFT JOIN usage ON usage.room_id = rooms.room_id
            UNION ALL SELECT NULL, NULL, files, bytes FROM usage WHERE room_id IS NULL
            ORDER BY room_id",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut report = AttachmentStorageUsage::default();
        for row in rows {
            let files = row.try_get("files")?;
            let bytes = row.try_get("bytes")?;
            report.total.files += files;
            report.total.bytes += bytes;
            if let Some(room_id) = row.try_get::<Option<String>, _>("room_id")? {
                report.rooms.push(RoomAttachmentStorageUsage {
                    room_id,
                    label: row.try_get("label")?,
                    files,
                    bytes,
                });
            } else {
                report.other = AttachmentStorageCount { files, bytes };
            }
        }
        Ok(report)
    }
}
