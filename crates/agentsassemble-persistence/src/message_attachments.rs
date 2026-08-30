use std::collections::BTreeSet;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, MAX_ATTACHMENT_BYTES, MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES,
    MAX_MESSAGE_ATTACHMENTS_PER_EVENT, MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX,
    MESSAGE_ATTACHMENT_ID_PREFIX, MESSAGE_ATTACHMENT_REFERENCE_PREFIX,
    MESSAGE_ATTACHMENT_VIEW_SUFFIX, RoomEvent, VOTE_QUESTION_CHARACTER_LIMIT,
    canonical_message_attachment_filename, clean_message, has_visible_text,
    is_message_attachment_id, require_message_write_authority,
};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    agent_lifecycle::load_session,
    asset_storage::enforce_storage_replacement,
    authority::load_active_participant,
    human_session_authority::revalidate_human_session,
    provider_turn_execution::load_execution_in,
    raster_assets::{is_safe_raster_content_type, validate_preserved_safe_raster},
    room_turns::support::{load_event, load_participant},
    room_user_identity::resolve_room_user_identity,
    turn_authority::active_turn_authority,
};

const PENDING_ATTACHMENT_TTL: Duration = Duration::hours(1);
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageAttachment {
    pub metadata: MessageAttachmentMetadata,
    pub content: Vec<u8>,
}

#[derive(Debug, Clone, Copy)]
pub struct ProviderAttachmentReadAuthority<'a> {
    pub room_id: &'a str,
    pub session_id: &'a str,
    pub turn_id: &'a str,
    pub input_up_to_seq: i64,
    pub turn_generation: u64,
    pub execution_id: &'a str,
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

pub(crate) fn message_has_visible_payload(event: &RoomEvent) -> Result<bool, PersistenceError> {
    let attachments = message_attachments_from_event(event)?;
    Ok(has_visible_text(&message_visible_text(event)?) || !attachments.is_empty())
}

pub(crate) fn message_visible_text(event: &RoomEvent) -> Result<String, PersistenceError> {
    let content = clean_message(event.content.as_deref().unwrap_or_default(), 12_000);
    if has_visible_text(&content) {
        return Ok(content);
    }
    if event.message_kind.as_deref() != Some("vote") {
        return Ok(String::new());
    }
    let question = event
        .extra
        .get("vote_question")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(invalid_message_state)?;
    let canonical = clean_message(question, VOTE_QUESTION_CHARACTER_LIMIT);
    if canonical != question || !has_visible_text(&canonical) {
        return Err(invalid_message_state());
    }
    Ok(canonical)
}

pub(crate) fn message_attachment_ids_from_events<'a>(
    events: impl IntoIterator<Item = &'a RoomEvent>,
) -> Result<Vec<String>, PersistenceError> {
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for event in events {
        for attachment in message_attachments_from_event(event)? {
            if seen.insert(attachment.id.clone()) {
                ids.push(attachment.id);
            }
        }
    }
    Ok(ids)
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
    /// Proves that the canonical local room human may currently upload a message attachment.
    ///
    /// # Errors
    ///
    /// Fails closed for stale, non-local, muted, or otherwise non-writable authority.
    pub async fn authorize_local_message_attachment_upload(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        current_local_message_principal(&mut transaction, room_id, user_id, participant_id).await?;
        transaction.commit().await?;
        Ok(())
    }

    /// Proves that a durable human session may currently upload a message attachment.
    ///
    /// # Errors
    ///
    /// Fails closed for expired, replaced, read-only, muted, or otherwise stale authority.
    pub async fn authorize_human_session_message_attachment_upload(
        &self,
        authorization: &HumanSessionAuthorization,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        require_current_message_writer(&mut transaction, current.principal()).await?;
        transaction.commit().await?;
        Ok(())
    }

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

    /// Stores one pending message attachment for the canonical local room human.
    ///
    /// # Errors
    ///
    /// Fails closed when the consumed grant identity is stale, non-local, muted, or otherwise
    /// cannot write the room, or when attachment validation and storage fail.
    pub async fn store_local_message_attachment(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        filename: &str,
        content_type: &str,
        content: Vec<u8>,
    ) -> Result<MessageAttachmentMetadata, PersistenceError> {
        let prepared = prepare_message_attachment(filename, content_type, content).await?;
        let mut transaction = self.pool.begin().await?;
        let principal =
            current_local_message_principal(&mut transaction, room_id, user_id, participant_id)
                .await?;
        let metadata = store_pending_in_transaction(&mut transaction, &principal, prepared).await?;
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

    /// Reads one bound attachment only while the canonical local human can reach its message.
    ///
    /// # Errors
    ///
    /// Fails closed for stale membership, malformed IDs, unreferenced bytes, or corrupt state.
    pub async fn bound_message_attachment(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        attachment_id: &str,
    ) -> Result<MessageAttachment, PersistenceError> {
        if !is_message_attachment_id(attachment_id) {
            return Err(message_attachment_missing());
        }
        let mut transaction = self.pool.begin().await?;
        resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        let attachment =
            read_bound_message_attachment(&mut transaction, room_id, attachment_id).await?;
        transaction.commit().await?;
        Ok(attachment)
    }

    /// Reads one bound attachment through exact current human-session provenance.
    ///
    /// # Errors
    ///
    /// Fails closed for expired or changed session authority, absent history permission,
    /// unreferenced bytes, or corrupt state.
    pub async fn bound_human_session_message_attachment(
        &self,
        authorization: &HumanSessionAuthorization,
        attachment_id: &str,
    ) -> Result<MessageAttachment, PersistenceError> {
        if !is_message_attachment_id(attachment_id) {
            return Err(message_attachment_missing());
        }
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        if !current.principal().capabilities.room_history {
            return Err(rejected(
                "permission_denied",
                "This room session cannot read message history.",
            ));
        }
        let attachment = read_bound_message_attachment(
            &mut transaction,
            &current.principal().room_id,
            attachment_id,
        )
        .await?;
        transaction.commit().await?;
        Ok(attachment)
    }

    /// Reads one attachment through the exact active Agent Session turn authority.
    ///
    /// # Errors
    ///
    /// Fails closed for stale turns or IDs outside the canonical inflight messages.
    pub async fn bound_provider_message_attachment(
        &self,
        authority: ProviderAttachmentReadAuthority<'_>,
        attachment_id: &str,
    ) -> Result<MessageAttachment, PersistenceError> {
        if !is_message_attachment_id(attachment_id) {
            return Err(message_attachment_missing());
        }
        let mut transaction = self.pool.begin().await?;
        let session =
            load_session(&mut transaction, authority.room_id, authority.session_id).await?;
        let participant = load_participant(
            &mut transaction,
            authority.room_id,
            &session.public.participant_id,
        )
        .await?;
        let execution = load_execution_in(
            &mut transaction,
            authority.room_id,
            authority.session_id,
            authority.turn_generation,
        )
        .await?;
        if participant.status != agentsassemble_domain::ParticipantStatus::Joined
            || participant.muted
            || session.public.active_turn_id != authority.turn_id
            || session.input_up_to_seq != authority.input_up_to_seq
            || session.turn_generation != authority.turn_generation
            || !active_turn_authority(&session).map_err(|_| stale_provider_turn())?
            || execution.execution_id != authority.execution_id
            || execution.turn_id != authority.turn_id
            || execution.participant_id != session.public.participant_id
            || execution.phase != crate::ProviderTurnExecutionPhase::StartDispatching
        {
            return Err(stale_provider_turn());
        }
        let mut inflight_events = Vec::with_capacity(session.inflight_inputs.len());
        for input in &session.inflight_inputs {
            inflight_events.push(
                load_event(&mut transaction, authority.room_id, &input.event_id)
                    .await?
                    .ok_or_else(message_attachment_missing)?,
            );
        }
        if !message_attachment_ids_from_events(inflight_events.iter())?
            .iter()
            .any(|candidate| candidate == attachment_id)
        {
            return Err(message_attachment_missing());
        }
        let attachment =
            read_bound_message_attachment(&mut transaction, authority.room_id, attachment_id)
                .await?;
        transaction.commit().await?;
        Ok(attachment)
    }
}

async fn current_local_message_principal(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    user_id: &str,
    participant_id: &str,
) -> Result<AuthenticatedPrincipal, PersistenceError> {
    let identity =
        resolve_room_user_identity(transaction, room_id, user_id, participant_id).await?;
    if identity.user_id != LOCAL_OPERATOR_USER_ID
        || identity.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
    {
        return Err(rejected(
            "permission_denied",
            "Only the canonical local room human may use a local attachment grant.",
        ));
    }
    let participant =
        load_active_participant(transaction, &identity.room_id, &identity.participant_id).await?;
    let principal = AuthenticatedPrincipal {
        principal_id: identity.user_id,
        participant_id: identity.participant_id,
        display_name: participant.display_name.clone(),
        room_id: identity.room_id,
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    require_message_write_authority(&principal, &participant).map_err(|error| {
        PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        }
    })?;
    Ok(principal)
}

async fn read_bound_message_attachment(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    attachment_id: &str,
) -> Result<MessageAttachment, PersistenceError> {
    let row = sqlx::query(
        "SELECT attachment.event_seq, attachment.filename, attachment.content_type, attachment.size, attachment.is_safe_image, length(attachment.content) AS content_length, event.event_json FROM room_message_attachments AS attachment INNER JOIN room_events AS event ON event.room_id = attachment.room_id AND event.seq = attachment.event_seq WHERE attachment.attachment_id = ? AND attachment.room_id = ? AND attachment.state = 'bound' AND attachment.pending_owner_user_id IS NULL AND attachment.expires_at IS NULL",
    )
    .bind(attachment_id)
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(message_attachment_missing)?;
    let stored = stored_metadata(attachment_id, &row)?;
    let event_seq = row.get::<i64, _>("event_seq");
    let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
    let event_attachments = message_attachments_from_event(&event)?;
    if event.room_id != room_id
        || event.seq != event_seq
        || !event.is_current_lobby_message()
        || !event_attachments.contains(&stored)
    {
        return Err(message_attachment_missing());
    }
    let content = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT content FROM room_message_attachments WHERE attachment_id = ? AND room_id = ? AND event_seq = ? AND state = 'bound'",
    )
    .bind(attachment_id)
    .bind(room_id)
    .bind(event_seq)
    .fetch_one(&mut **transaction)
    .await?;
    if content.len() != stored.size {
        return Err(invalid_attachment_state());
    }
    Ok(MessageAttachment {
        metadata: stored,
        content,
    })
}

fn stored_metadata(
    attachment_id: &str,
    row: &sqlx::sqlite::SqliteRow,
) -> Result<MessageAttachmentMetadata, PersistenceError> {
    let size =
        usize::try_from(row.get::<i64, _>("size")).map_err(|_| invalid_attachment_state())?;
    if row.get::<i64, _>("content_length") != i64::try_from(size).unwrap_or(i64::MAX) {
        return Err(invalid_attachment_state());
    }
    let metadata = metadata(
        attachment_id.to_owned(),
        row.get("filename"),
        row.get("content_type"),
        size,
        row.get::<i64, _>("is_safe_image") == 1,
    );
    if !canonical_metadata(&metadata) {
        return Err(invalid_attachment_state());
    }
    Ok(metadata)
}

async fn prepare_message_attachment(
    filename: &str,
    content_type: &str,
    content: Vec<u8>,
) -> Result<PreparedMessageAttachment, PersistenceError> {
    if content.is_empty() || content.len() > MAX_ATTACHMENT_BYTES {
        return Err(rejected(
            "attachment_too_large",
            "Message attachment must be between 1 byte and 10 MiB.",
        ));
    }
    let filename = canonical_message_attachment_filename(filename);
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
        usize::try_from(prepared.size).unwrap_or(MAX_ATTACHMENT_BYTES),
        prepared.is_safe_image,
    ))
}

fn normalize_content_type(value: &str, filename: &str) -> String {
    let candidate = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    if candidate.len() <= MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES
        && valid_content_type(&candidate)
    {
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
        url: format!("{MESSAGE_ATTACHMENT_REFERENCE_PREFIX}{id}{MESSAGE_ATTACHMENT_VIEW_SUFFIX}"),
        download_url: format!(
            "{MESSAGE_ATTACHMENT_REFERENCE_PREFIX}{id}{MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX}"
        ),
        id,
        filename,
        content_type,
        size,
        is_image,
    }
}

fn canonical_metadata(attachment: &MessageAttachmentMetadata) -> bool {
    is_message_attachment_id(&attachment.id)
        && canonical_message_attachment_filename(&attachment.filename) == attachment.filename
        && attachment.content_type.len() <= MAX_MESSAGE_ATTACHMENT_CONTENT_TYPE_BYTES
        && valid_content_type(&attachment.content_type)
        && (1..=MAX_ATTACHMENT_BYTES).contains(&attachment.size)
        && attachment.is_image == is_safe_raster_content_type(&attachment.content_type)
        && attachment.url
            == format!(
                "{MESSAGE_ATTACHMENT_REFERENCE_PREFIX}{}{MESSAGE_ATTACHMENT_VIEW_SUFFIX}",
                attachment.id
            )
        && attachment.download_url
            == format!(
                "{MESSAGE_ATTACHMENT_REFERENCE_PREFIX}{}{MESSAGE_ATTACHMENT_DOWNLOAD_SUFFIX}",
                attachment.id
            )
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

fn invalid_message_state() -> PersistenceError {
    rejected(
        "invalid_state",
        "Stored room message visible content is invalid.",
    )
}

fn message_attachment_missing() -> PersistenceError {
    rejected(
        "message_attachment_missing",
        "Message attachment was not found.",
    )
}

fn stale_provider_turn() -> PersistenceError {
    rejected(
        "stale_provider_turn",
        "The Agent Session attachment read no longer owns the active turn.",
    )
}
