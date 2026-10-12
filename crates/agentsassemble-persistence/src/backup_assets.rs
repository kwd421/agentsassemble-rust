//! Reconstructed asset bytes cross the same bounded decoder as live uploads.
use super::{BTreeMap, Cell, PersistenceError, invalid};
use crate::{message_attachments, persona_library, raster_assets};

fn text<'a>(values: &'a BTreeMap<String, Cell>, name: &str) -> Result<&'a str, PersistenceError> {
    values.get(name).and_then(Cell::text).ok_or_else(invalid)
}

fn integer(values: &BTreeMap<String, Cell>, name: &str) -> Result<i64, PersistenceError> {
    match values.get(name) {
        Some(Cell::Integer(value)) => Ok(*value),
        _ => Err(invalid()),
    }
}

pub(super) async fn validate(
    table: &str,
    values: &mut BTreeMap<String, Cell>,
    version: i64,
) -> Result<(), PersistenceError> {
    if table == "room_message_attachments" {
        require_message_metadata(values, version)?;
        return Ok(());
    }
    let column = match table {
        "persona_assets" => "thumbnail_png",
        "profile_avatar_assets" | "agent_avatar_assets" | "room_appearance_assets" => "content",
        _ => return Ok(()),
    };
    if table == "persona_assets" {
        let card = serde_json::from_str(text(values, "card_json")?)?;
        let thumbnail = match values.get(column) {
            Some(Cell::Blob(bytes)) => Some(bytes.as_slice()),
            Some(Cell::Null) => None,
            _ => return Err(invalid()),
        };
        persona_library::validate_thumbnail(&card, thumbnail)?;
        if thumbnail.is_none() {
            return Ok(());
        }
    }
    let Some(Cell::Blob(content)) = values.remove(column) else {
        return Err(invalid());
    };
    let content_type = if table == "persona_assets" {
        "image/png".to_owned()
    } else {
        text(values, "content_type")?.to_owned()
    };
    if table != "persona_assets" {
        raster_assets::validate_stored_raster(
            &content_type,
            integer(values, "size")?,
            i64::try_from(content.len()).map_err(|_| invalid())?,
            text(values, "created_at")?,
        )?;
        let filename = text(values, "filename")?;
        if raster_assets::sanitize_filename(filename) != filename {
            return Err(invalid());
        }
    }
    let (content, _) =
        raster_assets::validate_preserved_safe_raster(&content_type, content).await?;
    values.insert(column.into(), Cell::Blob(content));
    Ok(())
}

fn require_message_metadata(
    values: &BTreeMap<String, Cell>,
    version: i64,
) -> Result<message_attachments::MessageAttachmentMetadata, PersistenceError> {
    let metadata = message_attachments::metadata(
        text(values, "attachment_id")?.into(),
        text(values, "filename")?.into(),
        text(values, "content_type")?.into(),
        usize::try_from(integer(values, "size")?).map_err(|_| invalid())?,
        integer(values, "is_safe_image")? == 1,
    );
    if !message_attachments::canonical_metadata(&metadata)
        || (version < 92 && metadata.size > agentsassemble_domain::MAX_ATTACHMENT_BYTES)
    {
        return Err(invalid());
    }
    Ok(metadata)
}

// Raster validation retains its existing decoder budget and whole-input boundary.
// Non-raster room files are copied only in pieces, never assembled by archives.
pub(super) async fn validate_chunked(
    connection: &mut sqlx::SqliteConnection,
) -> Result<(), PersistenceError> {
    use sqlx::Row;
    let mut previous = String::new();
    loop {
        let row = sqlx::query(
            "SELECT attachment_id,content_type,size FROM room_message_attachments
            WHERE is_safe_image=1 AND attachment_id>? ORDER BY attachment_id LIMIT 1",
        )
        .bind(&previous)
        .fetch_optional(&mut *connection)
        .await?;
        let Some(row) = row else { break };
        let id: String = row.get("attachment_id");
        let size = usize::try_from(row.get::<i64, _>("size")).map_err(|_| invalid())?;
        let content = crate::room_file_content::RoomFileContent::Message
            .collect(connection, &id, size)
            .await?;
        raster_assets::validate_preserved_safe_raster(row.get("content_type"), content).await?;
        previous = id;
    }
    Ok(())
}

// Old archive cells are never fetched whole. Only the existing raster decoder
// boundary assembles compressed input; ordinary bytes copy inside SQLite.
pub(super) async fn validate_historical_file(
    connection: &mut sqlx::SqliteConnection,
    values: &BTreeMap<String, Cell>,
    version: i64,
) -> Result<(), PersistenceError> {
    let metadata = require_message_metadata(values, version)?;
    if integer(values, "archive_content_length")? != metadata.size as i64 {
        return Err(invalid());
    }
    if metadata.is_image {
        let mut bytes = Vec::with_capacity(metadata.size);
        for sequence in 0..metadata
            .size
            .div_ceil(crate::room_file_content::PIECE_BYTES)
        {
            bytes.extend(
                crate::room_file_content::RoomFileContent::read_archive_piece(
                    connection,
                    &metadata.id,
                    sequence,
                )
                .await?,
            );
        }
        raster_assets::validate_preserved_safe_raster(&metadata.content_type, bytes).await?;
    }
    Ok(())
}
