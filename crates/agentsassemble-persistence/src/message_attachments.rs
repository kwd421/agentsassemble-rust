use std::collections::BTreeSet;

use agentsassemble_domain::{
    AuthenticatedPrincipal, MAX_MESSAGE_ATTACHMENTS_PER_EVENT, MAX_RASTER_BYTES,
    MESSAGE_ATTACHMENT_ID_PREFIX, RoomEvent, is_message_attachment_id,
    require_message_write_authority,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    asset_storage::enforce_storage_replacement,
    authority::load_active_participant,
    human_session_authority::revalidate_human_session,
    raster_assets::{is_safe_raster_content_type, validate_preserved_safe_raster},
};

const PENDING_ATTACHMENT_TTL: Duration = Duration::hours(1);
const MAX_CONTENT_TYPE_BYTES: usize = 127;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAttachmentMetadata {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
    pub is_image: bool,
    pub url: String,
    pub download_url: String,
}

pub(crate) fn message_attachments_from_event(
    event: &RoomEvent,
) -> Result<Vec<MessageAttachmentMetadata>, PersistenceError> {
    let Some(value) = event.extra.get("attachments") else {
        return Ok(Vec::new());
    };
    let attachments = Vec::<MessageAttachmentMetadata>::deserialize(value)
        .map_err(|_| invalid_attachment_state())?;
    if attachments.len() > MAX_MESSAGE_ATTACHMENTS_PER_EVENT {
        return Err(invalid_attachment_state());
    }
    let mut ids = BTreeSet::new();
    for attachment in &attachments {
        if !ids.insert(attachment.id.as_str()) || !canonical_metadata(attachment) {
            return Err(invalid_attachment_state());
        }
    }
    Ok(attachments)
}

pub(crate) async fn prepare_message_attachment_bindings(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    attachment_ids: &[String],
    now: i64,
) -> Result<Vec<MessageAttachmentMetadata>, PersistenceError> {
    let mut attachments = Vec::with_capacity(attachment_ids.len());
    for attachment_id in attachment_ids {
        let row = sqlx::query(
            "SELECT room_id, pending_owner_user_id, event_seq, filename, content_type, size, is_safe_image, state, expires_at FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(attachment_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or_else(attachment_unavailable)?;
        if row.get::<String, _>("room_id") != principal.room_id
            || row.get::<Option<String>, _>("pending_owner_user_id")
                != Some(principal.principal_id.clone())
            || row.get::<Option<i64>, _>("event_seq").is_some()
            || row.get::<String, _>("state") != "pending"
            || row
                .get::<Option<i64>, _>("expires_at")
                .is_none_or(|expires_at| expires_at <= now)
        {
            return Err(attachment_unavailable());
        }
        let size =
            usize::try_from(row.get::<i64, _>("size")).map_err(|_| invalid_attachment_state())?;
        attachments.push(metadata(
            attachment_id.clone(),
            row.get("filename"),
            row.get("content_type"),
            size,
            row.get("is_safe_image"),
        ));
    }
    Ok(attachments)
}

pub(crate) async fn bind_message_attachments(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    attachment_ids: &[String],
    event_seq: i64,
    now: i64,
) -> Result<(), PersistenceError> {
    for attachment_id in attachment_ids {
        let result = sqlx::query(
            "UPDATE room_message_attachments SET pending_owner_user_id = NULL, event_seq = ?, state = 'bound', expires_at = NULL WHERE attachment_id = ? AND room_id = ? AND pending_owner_user_id = ? AND event_seq IS NULL AND state = 'pending' AND expires_at > ?",
        )
        .bind(event_seq)
        .bind(attachment_id)
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(now)
        .execute(&mut **transaction)
        .await?;
        if result.rows_affected() != 1 {
            return Err(attachment_unavailable());
        }
    }
    Ok(())
}

struct PreparedMessageAttachment {
    filename: String,
    content_type: String,
    content: Vec<u8>,
    size: i64,
    is_safe_image: bool,
}

impl SqliteStore {
    /// Stores one pending message attachment for the current writable room principal.
    ///
    /// Arbitrary files retain their exact bytes. Only bounded decoded safe raster formats are
    /// classified for inline rendering.
    ///
    /// # Errors
    ///
    /// Fails closed on stale or non-writable room authority, invalid safe-raster bytes, absolute
    /// storage exhaustion, or a persistence failure.
    pub async fn store_message_attachment(
        &self,
        principal: &AuthenticatedPrincipal,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<MessageAttachmentMetadata, PersistenceError> {
        let prepared = prepare_message_attachment(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        require_current_message_writer(&mut transaction, principal).await?;
        let metadata = store_pending_in_transaction(&mut transaction, principal, prepared).await?;
        transaction.commit().await?;
        Ok(metadata)
    }

    /// Stores one pending message attachment after exact human-session revalidation.
    ///
    /// # Errors
    ///
    /// Fails closed when the consumed grant no longer matches its durable session, the current
    /// participant is muted or read-only, or attachment validation and storage fail.
    pub async fn store_human_session_message_attachment(
        &self,
        authorization: &HumanSessionAuthorization,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<MessageAttachmentMetadata, PersistenceError> {
        let prepared = prepare_message_attachment(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        require_current_message_writer(&mut transaction, current.principal()).await?;
        let metadata =
            store_pending_in_transaction(&mut transaction, current.principal(), prepared).await?;
        transaction.commit().await?;
        Ok(metadata)
    }
}

async fn prepare_message_attachment(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
) -> Result<PreparedMessageAttachment, PersistenceError> {
    if content.is_empty() || content.len() > MAX_RASTER_BYTES {
        return Err(rejected(
            "attachment_too_large",
            "Message attachment must be between 1 byte and 10 MiB.",
        ));
    }
    let filename = sanitize_message_filename(filename);
    let content_type = normalize_content_type(content_type, &filename);
    let (content, is_safe_image) = validate_preserved_safe_raster(&content_type, content).await?;
    let size = i64::try_from(content.len()).map_err(|_| {
        rejected(
            "attachment_too_large",
            "Message attachment exceeds the supported size.",
        )
    })?;
    Ok(PreparedMessageAttachment {
        filename,
        content_type,
        content,
        size,
        is_safe_image,
    })
}

async fn require_current_message_writer(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
) -> Result<(), PersistenceError> {
    let participant =
        load_active_participant(transaction, &principal.room_id, &principal.participant_id).await?;
    require_message_write_authority(principal, &participant).map_err(|error| {
        PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        }
    })
}

async fn store_pending_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    prepared: PreparedMessageAttachment,
) -> Result<MessageAttachmentMetadata, PersistenceError> {
    let now = Utc::now();
    let created_at = now.timestamp();
    let expires_at = (now + PENDING_ATTACHMENT_TTL).timestamp();
    sqlx::query("DELETE FROM room_message_attachments WHERE state = 'pending' AND expires_at <= ?")
        .bind(created_at)
        .execute(&mut **transaction)
        .await?;
    enforce_storage_replacement(transaction, None, prepared.size).await?;

    let attachment_id = format!("{MESSAGE_ATTACHMENT_ID_PREFIX}{}", Uuid::new_v4().simple());
    sqlx::query(
        "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, ?, ?, NULL, ?, ?, ?, ?, ?, ?, 'pending', ?)",
    )
    .bind(&attachment_id)
    .bind(&principal.room_id)
    .bind(&principal.principal_id)
    .bind(&prepared.filename)
    .bind(&prepared.content_type)
    .bind(prepared.content)
    .bind(prepared.size)
    .bind(prepared.is_safe_image)
    .bind(created_at)
    .bind(expires_at)
    .execute(&mut **transaction)
    .await?;
    Ok(metadata(
        attachment_id,
        prepared.filename,
        prepared.content_type,
        usize::try_from(prepared.size).unwrap_or(MAX_RASTER_BYTES),
        prepared.is_safe_image,
    ))
}

fn sanitize_message_filename(value: &str) -> String {
    let normalized = value.replace('\\', "/");
    let name = normalized.rsplit('/').next().unwrap_or_default();
    let name: String = name
        .chars()
        .filter(|character| !character.is_control() && !matches!(character, '/' | '\\'))
        .collect::<String>()
        .trim()
        .chars()
        .take(120)
        .collect();
    if name.is_empty() || matches!(name.as_str(), "." | "..") {
        "attachment.bin".to_owned()
    } else {
        name
    }
}

fn normalize_content_type(value: &str, filename: &str) -> String {
    let candidate = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if candidate.len() <= MAX_CONTENT_TYPE_BYTES && valid_content_type(&candidate) {
        candidate
    } else {
        mime_guess::from_path(filename)
            .first_raw()
            .unwrap_or("application/octet-stream")
            .to_owned()
    }
}

fn valid_content_type(value: &str) -> bool {
    let Some((top, subtype)) = value.split_once('/') else {
        return false;
    };
    !top.is_empty()
        && !subtype.is_empty()
        && top.bytes().chain(subtype.bytes()).all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'.' | b'+' | b'-')
        })
}

fn metadata(
    id: String,
    filename: String,
    content_type: String,
    size: usize,
    is_image: bool,
) -> MessageAttachmentMetadata {
    MessageAttachmentMetadata {
        url: format!("/api/attachments/{id}?view=1"),
        download_url: format!("/api/attachments/{id}?download=1"),
        id,
        filename,
        content_type,
        size,
        is_image,
    }
}

fn canonical_metadata(attachment: &MessageAttachmentMetadata) -> bool {
    is_message_attachment_id(&attachment.id)
        && sanitize_message_filename(&attachment.filename) == attachment.filename
        && attachment.content_type.len() <= MAX_CONTENT_TYPE_BYTES
        && valid_content_type(&attachment.content_type)
        && (1..=MAX_RASTER_BYTES).contains(&attachment.size)
        && attachment.is_image == is_safe_raster_content_type(&attachment.content_type)
        && attachment.url == format!("/api/attachments/{}?view=1", attachment.id)
        && attachment.download_url == format!("/api/attachments/{}?download=1", attachment.id)
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

fn attachment_unavailable() -> PersistenceError {
    rejected(
        "attachment_unavailable",
        "A message attachment is unavailable or no longer pending.",
    )
}

fn invalid_attachment_state() -> PersistenceError {
    rejected(
        "invalid_state",
        "Stored message attachment metadata is invalid.",
    )
}
