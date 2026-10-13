//! Process-local human upload custody; TEMP bytes never become archive input.
use agentsassemble_domain::{AuthenticatedPrincipal, canonical_message_attachment_filename};
use chrono::Utc;
use serde::Serialize;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use uuid::Uuid;

use crate::message_attachments as attachments;
use crate::{MessageAttachmentMetadata, PersistenceError, RoomSessionAuthorization, SqliteStore};

pub const MESSAGE_UPLOAD_PIECE_BYTES: usize = crate::room_file_content::PIECE_BYTES;

#[derive(Clone)]
pub enum MessageUploadAuthority {
    Local {
        room_id: String,
        user_id: String,
        participant_id: String,
    },
    Session(Box<RoomSessionAuthorization>),
    Provider(Box<crate::ProviderFileAuthority>),
    Attendee(Box<crate::AttendeeFileAuthority>),
}

impl MessageUploadAuthority {
    #[must_use]
    pub fn identity(&self) -> (&str, &str, &str) {
        match self {
            Self::Provider(p) => (&p.room_id, &p.session_id, &p.participant_id),
            Self::Attendee(a) => (&a.room_id, &a.session_id, &a.participant_id),
            Self::Local {
                room_id,
                user_id,
                participant_id,
            } => (room_id, user_id, participant_id),
            Self::Session(session) => {
                let p = session.principal();
                (&p.room_id, &p.principal_id, &p.participant_id)
            }
        }
    }
    #[must_use]
    pub fn same_custody(&self, other: &Self) -> bool {
        self.identity() == other.identity()
            && match (self, other) {
                (Self::Local { .. }, Self::Local { .. }) => true,
                (Self::Provider(a), Self::Provider(b)) => a == b,
                (Self::Attendee(a), Self::Attendee(b)) => a == b,
                (Self::Session(a), Self::Session(b)) => {
                    a.session_fingerprint() == b.session_fingerprint()
                }
                _ => false,
            }
    }
    async fn resolve(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        match self {
            Self::Provider(p) => p.resolve(tx).await,
            Self::Attendee(a) => a.resolve(tx).await,
            Self::Local {
                room_id,
                user_id,
                participant_id,
            } => {
                attachments::current_local_message_principal(tx, room_id, user_id, participant_id)
                    .await
            }
            Self::Session(session) => {
                let principal = session.mutation_authority().resolve(tx).await?;
                attachments::require_current_message_writer(tx, &principal).await?;
                Ok(principal.into_owned())
            }
        }
    }
}

#[derive(Clone, Serialize)]
pub struct MessageUploadStatus {
    pub handle: String,
    pub attachment_id: String,
    pub piece_size: usize,
    pub expires_at: i64,
    pub declared_size: usize,
    pub accepted_offset: usize,
    pub attachment: Option<MessageAttachmentMetadata>,
}

pub(crate) async fn initialize_connection(
    connection: &mut SqliteConnection,
) -> Result<(), sqlx::Error> {
    // FILE is required: TEMP_STORE=3 would silently turn 64 large transfers into RAM.
    let memory_only: bool = sqlx::query_scalar("SELECT sqlite_compileoption_used('TEMP_STORE=3')")
        .fetch_one(&mut *connection)
        .await?;
    if memory_only {
        return Err(sqlx::Error::Protocol(
            "Disk-backed upload staging is unavailable".into(),
        ));
    }
    for statement in [
        "PRAGMA temp_store=FILE",
        "CREATE TEMP TABLE message_uploads (
            handle TEXT PRIMARY KEY, attachment_id TEXT NOT NULL UNIQUE,
            room_id TEXT NOT NULL, room_uid TEXT NOT NULL, user_id TEXT NOT NULL, participant_id TEXT NOT NULL,
            operation_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL,
            size INTEGER NOT NULL CHECK(size BETWEEN 1 AND 52428800),
            accepted INTEGER NOT NULL DEFAULT 0 CHECK(accepted BETWEEN 0 AND size),
            expires_at INTEGER NOT NULL, finished INTEGER NOT NULL DEFAULT 0,
            is_image INTEGER NOT NULL DEFAULT 0,
            UNIQUE(room_id,user_id,participant_id,operation_id))",
        "CREATE TEMP TABLE message_upload_pieces (
            handle TEXT NOT NULL REFERENCES message_uploads(handle) ON DELETE CASCADE,
            sequence INTEGER NOT NULL CHECK(sequence BETWEEN 0 AND 799),
            content BLOB NOT NULL CHECK(typeof(content)='blob' AND length(content) BETWEEN 1 AND 65536),
            PRIMARY KEY(handle,sequence)) WITHOUT ROWID",
        "PRAGMA temp.cache_size=-256",
    ] {
        sqlx::query(statement).execute(&mut *connection).await?;
    }
    Ok(())
}

impl SqliteStore {
    /// Reserve declared storage for an already authenticated exact human custody.
    /// # Errors
    /// Rejects invalid declarations, changed authority, quota exhaustion or conflicting retries.
    pub async fn begin_message_upload(
        &self,
        authority: &MessageUploadAuthority,
        operation_id: &str,
        filename: &str,
        content_type: &str,
        size: usize,
    ) -> Result<MessageUploadStatus, PersistenceError> {
        if operation_id.is_empty()
            || operation_id.len() > 128
            || filename.len() > 4096
            || content_type.len() > 256
        {
            return Err(rejected("upload_declaration_invalid"));
        }
        let filename = canonical_message_attachment_filename(filename);
        let content_type = attachments::normalize_content_type(content_type, &filename);
        let mut tx = self.pool.begin().await?;
        let p = authority.resolve(&mut tx).await?;
        expire_in(&mut tx).await?;
        if let Some(row) = sqlx::query("SELECT * FROM temp.message_uploads WHERE room_id=? AND user_id=? AND participant_id=? AND operation_id=?")
            .bind(&p.room_id).bind(&p.principal_id).bind(&p.participant_id).bind(operation_id)
            .fetch_optional(&mut *tx).await? {
            if row.get::<i64,_>("size") != size as i64 || row.get::<String,_>("filename") != filename
                || row.get::<String,_>("content_type") != content_type {
                return Err(rejected("upload_operation_conflict"));
            }
            require_custody(&mut tx,authority,&row.get::<String,_>("handle")).await?;
            let result = status_row(&row)?;
            tx.commit().await?;
            return Ok(result);
        }
        attachments::require_file_size(size, agentsassemble_domain::MAX_ROOM_FILE_BYTES)?;
        attachments::require_storage_capacity(&mut tx, &p, size as i64).await?;
        let handle = Uuid::new_v4().simple().to_string();
        let id = format!("ma_{}", Uuid::new_v4().simple());
        let expiry = Utc::now().timestamp() + 3600;
        sqlx::query("INSERT INTO temp.message_uploads(handle,attachment_id,room_id,user_id,participant_id,operation_id,filename,content_type,size,expires_at,room_uid) VALUES(?,?,?,?,?,?,?,?,?,?,(SELECT json_extract(room_json,'$.room_uid') FROM rooms WHERE room_id=?))")
            .bind(&handle).bind(&id).bind(&p.room_id).bind(&p.principal_id).bind(&p.participant_id)
            .bind(operation_id).bind(filename).bind(content_type).bind(size as i64).bind(expiry).bind(&p.room_id)
            .execute(&mut *tx).await?;
        let status = status_in(&mut tx, &handle).await?;
        tx.commit().await?;
        Ok(status)
    }

    /// Append at most one bounded input piece, retaining a partial tail for exact resume.
    /// # Errors
    /// Refuses gaps, overlaps, replay, overflow, completed transfers or lost authority.
    pub async fn append_message_upload(
        &self,
        authority: &MessageUploadAuthority,
        handle: &str,
        offset: usize,
        bytes: &[u8],
    ) -> Result<usize, PersistenceError> {
        if bytes.is_empty() || bytes.len() > MESSAGE_UPLOAD_PIECE_BYTES {
            return Err(rejected("upload_piece_invalid"));
        }
        let mut tx = self.pool.begin().await?;
        authority.resolve(&mut tx).await?;
        let status = status_in(&mut tx, handle).await?;
        require_custody(&mut tx, authority, handle).await?;
        if status.attachment.is_some() || status.accepted_offset != offset {
            return Err(rejected("upload_offset_conflict"));
        }
        let next = offset
            .checked_add(bytes.len())
            .filter(|n| *n <= status.declared_size)
            .ok_or_else(|| rejected("upload_size_exceeded"))?;
        let mut consumed = 0;
        let mut position = offset;
        while consumed < bytes.len() {
            let sequence = position / MESSAGE_UPLOAD_PIECE_BYTES;
            let tail = position % MESSAGE_UPLOAD_PIECE_BYTES;
            let mut piece = if tail == 0 {
                Vec::new()
            } else {
                sqlx::query_scalar::<_, Vec<u8>>(
                    "SELECT content FROM temp.message_upload_pieces WHERE handle=? AND sequence=?",
                )
                .bind(handle)
                .bind(sequence as i64)
                .fetch_one(&mut *tx)
                .await?
            };
            let take = (MESSAGE_UPLOAD_PIECE_BYTES - tail).min(bytes.len() - consumed);
            piece.extend_from_slice(&bytes[consumed..consumed + take]);
            sqlx::query("INSERT INTO temp.message_upload_pieces(handle,sequence,content) VALUES(?,?,?) ON CONFLICT(handle,sequence) DO UPDATE SET content=excluded.content")
                .bind(handle).bind(sequence as i64).bind(piece).execute(&mut *tx).await?;
            consumed += take;
            position += take;
        }
        sqlx::query("UPDATE temp.message_uploads SET accepted=? WHERE handle=?")
            .bind(next as i64)
            .bind(handle)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(next)
    }

    /// Inspect accepted offset or the unique committed pending result.
    /// # Errors
    /// Rejects expired, missing or mismatched custody and current authority loss.
    pub async fn message_upload_status(
        &self,
        authority: &MessageUploadAuthority,
        handle: &str,
    ) -> Result<MessageUploadStatus, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        authority.resolve(&mut tx).await?;
        require_custody(&mut tx, authority, handle).await?;
        let result = status_in(&mut tx, handle).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Discard only incomplete private custody; committed attachments retain their owner.
    /// # Errors
    /// Propagates database failure.
    pub async fn discard_message_upload(&self, handle: &str) -> Result<(), PersistenceError> {
        sqlx::query("DELETE FROM temp.message_uploads WHERE handle=? AND finished=0")
            .bind(handle)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Validate outside the writer, then atomically exchange reservation for pending custody.
    /// The service serializes finish with data/cancel and shields it from caller cancellation.
    /// # Errors
    /// Refuses incomplete length, invalid raster, changed authority or lowered policy/quota.
    pub async fn finish_message_upload(
        &self,
        authority: &MessageUploadAuthority,
        handle: &str,
    ) -> Result<MessageUploadStatus, PersistenceError> {
        let original = self.message_upload_status(authority, handle).await?;
        if original.attachment.is_some() {
            return Ok(original);
        }
        if original.accepted_offset != original.declared_size {
            return Err(rejected("upload_incomplete"));
        }
        let row =
            sqlx::query("SELECT filename,content_type FROM temp.message_uploads WHERE handle=?")
                .bind(handle)
                .fetch_one(&self.pool)
                .await?;
        let filename: String = row.get("filename");
        let content_type: String = row.get("content_type");
        let is_image = crate::raster_assets::is_safe_raster_content_type(&content_type);
        let preview = if is_image {
            let permit = crate::raster_assets::decode_permit().await?;
            let mut content = Vec::with_capacity(original.declared_size);
            for sequence in 0..original.declared_size.div_ceil(MESSAGE_UPLOAD_PIECE_BYTES) {
                let piece: Vec<u8> = sqlx::query_scalar(
                    "SELECT content FROM temp.message_upload_pieces WHERE handle=? AND sequence=?",
                )
                .bind(handle)
                .bind(sequence as i64)
                .fetch_one(&self.pool)
                .await?;
                content.extend_from_slice(&piece);
            }
            crate::raster_assets::upload_preview(&content_type, content, permit).await?
        } else {
            None
        };
        let mut tx = self.pool.begin().await?;
        let p = authority.resolve(&mut tx).await?;
        require_custody(&mut tx, authority, handle).await?;
        let current = status_in(&mut tx, handle).await?;
        if current.attachment.is_some() {
            return Ok(current);
        }
        // Exchanging the reservation and insertion share this transaction. Rollback
        // keeps the reservation intact if a changed policy rejects the commit.
        sqlx::query("UPDATE temp.message_uploads SET finished=1,is_image=? WHERE handle=?")
            .bind(is_image)
            .bind(handle)
            .execute(&mut *tx)
            .await?;
        attachments::require_storage_capacity(&mut tx, &p, original.declared_size as i64).await?;
        let created = Utc::now().timestamp();
        let participant_owned = matches!(
            p.client_kind,
            agentsassemble_domain::ClientKind::AgentBridge
                | agentsassemble_domain::ClientKind::RoomConnector
        );
        if participant_owned {
            sqlx::query("INSERT INTO room_connector_uploads(attachment_id,room_id,participant_id,filename,content_type,size,is_safe_image,created_at,expires_at) VALUES(?,?,?,?,?,?,?,?,?)")
                .bind(&original.attachment_id).bind(&p.room_id).bind(&p.participant_id).bind(&filename)
                .bind(&content_type).bind(original.declared_size as i64).bind(is_image).bind(created).bind(created+3600)
                .execute(&mut *tx).await?;
            sqlx::query("INSERT INTO room_connector_upload_chunks(attachment_id,sequence,content) SELECT ?,sequence,content FROM temp.message_upload_pieces WHERE handle=?")
                .bind(&original.attachment_id).bind(handle).execute(&mut *tx).await?;
        } else {
            sqlx::query("INSERT INTO room_message_attachments(attachment_id,room_id,pending_owner_user_id,filename,content_type,size,is_safe_image,created_at,state,expires_at) VALUES(?,?,?,?,?,?,?,?,'pending',?)")
            .bind(&original.attachment_id).bind(&p.room_id).bind(&p.principal_id).bind(&filename)
            .bind(&content_type).bind(original.declared_size as i64).bind(is_image).bind(created).bind(created+3600)
            .execute(&mut *tx).await?;
            sqlx::query("INSERT INTO room_message_attachment_chunks(attachment_id,sequence,content) SELECT ?,sequence,content FROM temp.message_upload_pieces WHERE handle=?")
            .bind(&original.attachment_id).bind(handle).execute(&mut *tx).await?;
        }
        if is_image {
            crate::message_attachment_previews::store(
                &mut tx,
                &original.attachment_id,
                preview,
                participant_owned,
            )
            .await?;
        }
        sqlx::query("DELETE FROM temp.message_upload_pieces WHERE handle=?")
            .bind(handle)
            .execute(&mut *tx)
            .await?;
        if let MessageUploadAuthority::Session(session) = authority {
            session.mutation_authority().record_success(&mut tx).await?;
        }
        let result = status_in(&mut tx, handle).await?;
        tx.commit().await?;
        Ok(result)
    }
}

async fn require_custody(
    tx: &mut Transaction<'_, Sqlite>,
    authority: &MessageUploadAuthority,
    handle: &str,
) -> Result<(), PersistenceError> {
    let (room, user, participant) = authority.identity();
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM temp.message_uploads AS upload JOIN rooms ON rooms.room_id=upload.room_id WHERE handle=? AND upload.room_id=? AND user_id=? AND participant_id=? AND upload.room_uid=json_extract(rooms.room_json,'$.room_uid'))")
        .bind(handle).bind(room).bind(user).bind(participant).fetch_one(&mut **tx).await?;
    if !exists {
        return Err(rejected("upload_missing"));
    }
    Ok(())
}
async fn status_in(
    tx: &mut Transaction<'_, Sqlite>,
    handle: &str,
) -> Result<MessageUploadStatus, PersistenceError> {
    let row = sqlx::query("SELECT * FROM temp.message_uploads WHERE handle=? AND expires_at>?")
        .bind(handle)
        .bind(Utc::now().timestamp())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| rejected("upload_missing"))?;
    status_row(&row)
}
fn status_row(row: &sqlx::sqlite::SqliteRow) -> Result<MessageUploadStatus, PersistenceError> {
    let id: String = row.try_get("attachment_id")?;
    let size = row.try_get::<i64, _>("size")? as usize;
    Ok(MessageUploadStatus {
        handle: row.try_get("handle")?,
        attachment_id: id.clone(),
        piece_size: MESSAGE_UPLOAD_PIECE_BYTES,
        expires_at: row.try_get("expires_at")?,
        declared_size: size,
        accepted_offset: row.try_get::<i64, _>("accepted")? as usize,
        attachment: if row.try_get::<bool, _>("finished")? {
            Some(attachments::metadata(
                id,
                row.try_get("filename")?,
                row.try_get("content_type")?,
                size,
                row.try_get("is_image")?,
            ))
        } else {
            None
        },
    })
}
pub(crate) async fn expire_in(tx: &mut Transaction<'_, Sqlite>) -> Result<(), PersistenceError> {
    sqlx::query("DELETE FROM temp.message_uploads WHERE expires_at<=? OR NOT EXISTS(SELECT 1 FROM rooms WHERE rooms.room_id=message_uploads.room_id AND json_extract(rooms.room_json,'$.room_uid')=message_uploads.room_uid)")
        .bind(Utc::now().timestamp())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
fn rejected(code: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: "Upload transfer request cannot be accepted.".into(),
    }
}

#[cfg(test)]
#[path = "message_upload_tests.rs"]
mod tests;
