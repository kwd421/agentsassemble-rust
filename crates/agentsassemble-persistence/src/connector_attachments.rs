//! Pending external uploads are owned by a room participant, not a human profile.
use super::{
    AuthenticatedPrincipal, MessageAttachmentMetadata, PersistenceError, PreparedMessageAttachment,
    Row, Sqlite, Transaction, attachment_unavailable, invalid_attachment_state, metadata,
};

pub(crate) const DDL: &str = "CREATE TABLE room_connector_uploads (
    attachment_id TEXT PRIMARY KEY,
    room_id TEXT NOT NULL, participant_id TEXT NOT NULL,
    filename TEXT NOT NULL, content_type TEXT NOT NULL,
    content BLOB NOT NULL CHECK(typeof(content) = 'blob'),
    size INTEGER NOT NULL CHECK(size > 0 AND size <= 10485760 AND length(content) = size),
    is_safe_image INTEGER NOT NULL CHECK(is_safe_image IN (0,1)),
    created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL CHECK(expires_at > created_at),
    FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE
)";

pub(super) async fn store(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    prepared: PreparedMessageAttachment,
    id: &str,
    created_at: i64,
    expires_at: i64,
) -> Result<MessageAttachmentMetadata, PersistenceError> {
    sqlx::query("INSERT INTO room_connector_uploads(attachment_id, room_id, participant_id, filename, content_type, content, size, is_safe_image, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
        .bind(id).bind(&principal.room_id).bind(&principal.participant_id)
        .bind(&prepared.filename).bind(&prepared.content_type).bind(prepared.content)
        .bind(prepared.size).bind(prepared.is_safe_image).bind(created_at).bind(expires_at)
        .execute(&mut **tx).await?;
    Ok(metadata(
        id.to_owned(),
        prepared.filename,
        prepared.content_type,
        usize::try_from(prepared.size).map_err(|_| invalid_attachment_state())?,
        prepared.is_safe_image,
    ))
}

pub(super) async fn prepare(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    ids: &[String],
    now: i64,
) -> Result<Vec<MessageAttachmentMetadata>, PersistenceError> {
    let mut attachments = Vec::with_capacity(ids.len());
    for id in ids {
        let row = sqlx::query("SELECT filename, content_type, size, is_safe_image FROM room_connector_uploads WHERE attachment_id = ? AND room_id = ? AND participant_id = ? AND expires_at > ?")
            .bind(id).bind(&principal.room_id).bind(&principal.participant_id).bind(now)
            .fetch_optional(&mut **tx).await?.ok_or_else(attachment_unavailable)?;
        attachments.push(metadata(
            id.clone(),
            row.get("filename"),
            row.get("content_type"),
            usize::try_from(row.get::<i64, _>("size")).map_err(|_| invalid_attachment_state())?,
            row.get("is_safe_image"),
        ));
    }
    Ok(attachments)
}

pub(super) async fn bind(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    ids: &[String],
    event_seq: i64,
    now: i64,
) -> Result<(), PersistenceError> {
    for id in ids {
        let result = sqlx::query("INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) SELECT attachment_id, room_id, NULL, ?, filename, content_type, content, size, is_safe_image, created_at, 'bound', NULL FROM room_connector_uploads WHERE attachment_id = ? AND room_id = ? AND participant_id = ? AND expires_at > ?")
            .bind(event_seq).bind(id).bind(&principal.room_id).bind(&principal.participant_id).bind(now)
            .execute(&mut **tx).await?;
        if result.rows_affected() != 1 {
            return Err(attachment_unavailable());
        }
        sqlx::query("DELETE FROM room_connector_uploads WHERE attachment_id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}
