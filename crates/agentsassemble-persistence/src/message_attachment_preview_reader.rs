//! Lazy cache work belongs to the same serialized writer and read authority.
use super::*;
use sqlx::Row;

impl SqliteStore {
    /// Open the preview variant with original authority and streamed delivery custody.
    /// Lazy decode admission also precedes original byte selection and stays with the worker.
    ///
    /// # Errors
    /// Rejects inaccessible files, exhausted admission and decode/storage failures.
    pub async fn open_message_attachment_preview_reader<T, G: Send + 'static>(
        &self,
        authority: MessageAttachmentReadAuthority,
        id: &str,
        admit: impl FnOnce(&str) -> Result<T, PersistenceError>,
        admit_generation: impl FnOnce(&str, usize) -> Result<G, PersistenceError>,
    ) -> Result<(MessageAttachmentReader, T), PersistenceError> {
        if !agentsassemble_domain::is_message_attachment_id(id) {
            return Err(super::super::message_attachment_missing());
        }
        let mut tx = self.pool.begin().await?;
        let (original, principal) = authority.metadata_in(&mut tx, id).await?;
        let lease = admit(&principal)?;
        if original.is_image && original.size > crate::raster_preview::MAX_BYTES {
            let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_message_attachment_previews WHERE attachment_id=?)")
                .bind(id).fetch_one(&mut *tx).await?;
            if !exists {
                let hold = admit_generation(&principal, original.size)?;
                let permit = crate::raster_assets::decode_permit().await?;
                let content = crate::room_file_content::RoomFileContent::Message
                    .collect(&mut tx, id, original.size)
                    .await?;
                let preview = crate::raster_assets::lazy_preview(
                    &original.content_type,
                    content,
                    permit,
                    hold,
                )
                .await?;
                crate::message_attachment_previews::store(&mut tx, id, preview, false).await?;
            }
        }
        let row = sqlx::query("SELECT content_type,length(content) AS size FROM room_message_attachment_previews WHERE attachment_id=? AND content IS NOT NULL")
            .bind(id).fetch_optional(&mut *tx).await?;
        let mut metadata = original.clone();
        let preview = row.is_some();
        if let Some(row) = row {
            metadata.content_type = row.try_get("content_type")?;
            metadata.size = usize::try_from(row.try_get::<i64, _>("size")?)
                .map_err(|_| invalid_attachment_state())?;
        }
        tx.commit().await?;
        Ok((
            MessageAttachmentReader {
                store: self.clone(),
                authority,
                metadata,
                original,
                preview,
            },
            lease,
        ))
    }
}

impl MessageAttachmentReader {
    pub(super) async fn preview_piece(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        sequence: usize,
    ) -> Result<Vec<u8>, PersistenceError> {
        let offset = sequence
            .checked_mul(crate::room_file_content::PIECE_BYTES)
            .filter(|offset| *offset < self.metadata.size)
            .ok_or_else(invalid_attachment_state)?;
        let bytes: Vec<u8> = sqlx::query_scalar("SELECT substr(content,?,65536) FROM room_message_attachment_previews WHERE attachment_id=? AND content_type=? AND length(content)=?")
            .bind((offset + 1) as i64).bind(&self.metadata.id).bind(&self.metadata.content_type)
            .bind(self.metadata.size as i64).fetch_optional(&mut **tx).await?.ok_or_else(invalid_attachment_state)?;
        if bytes.len() != (self.metadata.size - offset).min(crate::room_file_content::PIECE_BYTES) {
            return Err(invalid_attachment_state());
        }
        Ok(bytes)
    }
}
