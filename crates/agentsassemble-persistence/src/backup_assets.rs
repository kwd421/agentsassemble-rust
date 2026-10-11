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
) -> Result<(), PersistenceError> {
    let column = match table {
        "persona_assets" => "thumbnail_png",
        "profile_avatar_assets"
        | "agent_avatar_assets"
        | "room_appearance_assets"
        | "room_message_attachments" => "content",
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
    if table == "room_message_attachments" {
        let metadata = message_attachments::metadata(
            text(values, "attachment_id")?.into(),
            text(values, "filename")?.into(),
            content_type.clone(),
            usize::try_from(integer(values, "size")?).map_err(|_| invalid())?,
            integer(values, "is_safe_image")? == 1,
        );
        if !message_attachments::canonical_metadata(&metadata) || metadata.size != content.len() {
            return Err(invalid());
        }
    } else if table != "persona_assets" {
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
