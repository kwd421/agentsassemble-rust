use agentsassemble_domain::{
    CHANNEL_MESSAGE_EVENT_TYPE, MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS,
    MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS, MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS, RoomEvent,
    casefold_message_search_text, clean_message_search_value,
    compact_casefolded_message_search_text, room_event_is_owner_only,
};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

use crate::{
    PersistenceError,
    message_attachments::{message_attachments_from_event, message_visible_text},
};

pub(crate) struct SearchableRoomMessage {
    pub(crate) channel_id: String,
    pub(crate) author: String,
    pub(crate) content: String,
    pub(crate) attachment_filenames: Vec<String>,
    search_text: String,
    compact_text: String,
}

pub(crate) async fn index_room_message(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    let Some(message) = searchable_room_message(event)? else {
        return Ok(());
    };
    let created_at_nanos = canonical_created_at_nanos(event)?;
    sqlx::query(
        "INSERT INTO room_message_search_records(\
            room_id, event_seq, event_id, created_at_nanos, search_text, compact_text\
         ) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.room_id)
    .bind(event.seq)
    .bind(&event.id)
    .bind(created_at_nanos)
    .bind(message.search_text)
    .bind(message.compact_text)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn replace_room_message_index(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    remove_room_message_index(transaction, event).await?;
    index_room_message(transaction, event).await
}

pub(crate) async fn remove_room_message_index(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    let result = sqlx::query(
        "DELETE FROM room_message_search_records WHERE room_id = ? AND event_seq = ? AND event_id = ?",
    )
    .bind(&event.room_id)
    .bind(event.seq)
    .bind(&event.id)
    .execute(&mut **transaction)
    .await?;
    if result.rows_affected() != 1 {
        return Err(invalid_search_event());
    }
    Ok(())
}

// Account removal changes the index's author prefix, preserving the exact indexed
// message/attachment suffix inside SQLite. No full message/replay payload is exported.
pub(crate) async fn anonymize_search_author(
    transaction: &mut Transaction<'_, Sqlite>,
    id: i64,
) -> Result<(), PersistenceError> {
    use sqlx::Row;
    let row=sqlx::query("SELECT instr(search_text,char(10)) AS boundary,substr(search_text,1,min(instr(search_text,char(10))-1,512)) AS author FROM room_message_search_records WHERE id=?")
        .bind(id).fetch_one(&mut **transaction).await?;
    let boundary: i64 = row.try_get("boundary")?;
    if !(2..=513).contains(&boundary) {
        return Err(invalid_search_event());
    }
    let prefix = compact_casefolded_message_search_text(row.try_get("author")?);
    let name = casefold_message_search_text(crate::DEPARTED_USER_NAME);
    let compact = compact_casefolded_message_search_text(&name);
    let prefix_chars = i64::try_from(prefix.chars().count()).map_err(|_| invalid_search_event())?;
    let changed=sqlx::query("UPDATE room_message_search_records SET search_text=?||substr(search_text,?),compact_text=?||substr(compact_text,?) WHERE id=? AND substr(compact_text,1,?)=?")
        .bind(name).bind(boundary).bind(compact).bind(prefix_chars+1).bind(id).bind(prefix_chars).bind(prefix).execute(&mut **transaction).await?;
    if changed.rows_affected() != 1 {
        return Err(invalid_search_event());
    }
    sqlx::query("DELETE FROM room_message_search_phrase WHERE rowid=?")
        .bind(id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO room_message_search_phrase(rowid,search_text) SELECT id,search_text FROM room_message_search_records WHERE id=?").bind(id).execute(&mut **transaction).await?;
    Ok(())
}

pub(crate) fn searchable_room_message(
    event: &RoomEvent,
) -> Result<Option<SearchableRoomMessage>, PersistenceError> {
    if room_event_is_owner_only(event) {
        return Ok(None);
    }
    let channel_id = if event.is_current_lobby_message() {
        "lobby"
    } else if event.event_type == CHANNEL_MESSAGE_EVENT_TYPE
        && event.extra.get("message_deleted") != Some(&Value::Bool(true))
    {
        event
            .extra
            .get("channel_id")
            .and_then(Value::as_str)
            .filter(|value| agentsassemble_domain::is_custom_channel_id(value))
            .ok_or_else(invalid_search_event)?
    } else {
        return Ok(None);
    };
    let author = event
        .display_name
        .as_deref()
        .map(|value| clean_message_search_value(value, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS))
        .filter(|value| !value.is_empty())
        .or_else(|| {
            event
                .extra
                .get("name")
                .and_then(Value::as_str)
                .map(|value| {
                    clean_message_search_value(value, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS)
                })
                .filter(|value| !value.is_empty())
        })
        .or_else(|| {
            let value = clean_message_search_value(
                &event.actor.participant_id,
                MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS,
            );
            (!value.is_empty()).then_some(value)
        })
        .unwrap_or_else(|| "Room".to_owned());
    let content = clean_message_search_value(
        &message_visible_text(event)?,
        MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS,
    );
    let attachment_filenames = message_attachments_from_event(event)?
        .into_iter()
        .map(|attachment| {
            clean_message_search_value(
                &attachment.filename,
                MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS,
            )
        })
        .collect::<Vec<_>>();
    if content.is_empty() && attachment_filenames.is_empty() {
        return Ok(None);
    }
    let mut values = Vec::with_capacity(attachment_filenames.len() + 2);
    values.push(author.clone());
    if !content.is_empty() {
        values.push(content.clone());
    }
    values.extend(attachment_filenames.iter().cloned());
    let search_text = casefold_message_search_text(&values.join("\n"));
    let compact_text = compact_casefolded_message_search_text(&search_text);
    Ok(Some(SearchableRoomMessage {
        channel_id: channel_id.to_owned(),
        author,
        content,
        attachment_filenames,
        search_text,
        compact_text,
    }))
}

pub(crate) fn canonical_created_at_nanos(event: &RoomEvent) -> Result<i64, PersistenceError> {
    event
        .created_at
        .timestamp_nanos_opt()
        .filter(|value| *value > 0)
        .ok_or_else(invalid_search_event)
}

fn invalid_search_event() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state".into(),
        message: "The canonical message cannot be indexed.".to_owned(),
    }
}
