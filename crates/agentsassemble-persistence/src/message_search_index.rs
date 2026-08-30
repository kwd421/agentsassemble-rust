use agentsassemble_domain::{
    MAX_MESSAGE_ATTACHMENT_FILENAME_CHARACTERS, MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS,
    MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS, RoomEvent, casefold_message_search_text,
    clean_message_search_value, compact_casefolded_message_search_text, room_event_is_owner_only,
};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, message_attachments::message_attachments_from_event};

pub(crate) struct SearchableLobbyMessage {
    pub(crate) author: String,
    pub(crate) content: String,
    pub(crate) attachment_filenames: Vec<String>,
    search_text: String,
    compact_text: String,
}

pub(crate) async fn index_lobby_message(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    let Some(message) = searchable_lobby_message(event)? else {
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

pub(crate) fn searchable_lobby_message(
    event: &RoomEvent,
) -> Result<Option<SearchableLobbyMessage>, PersistenceError> {
    if !event.is_current_lobby_message() || room_event_is_owner_only(event) {
        return Ok(None);
    }
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
        event.content.as_deref().unwrap_or_default(),
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
    let mut values = Vec::with_capacity(attachment_filenames.len() + 2);
    values.push(author.clone());
    if !content.is_empty() {
        values.push(content.clone());
    }
    values.extend(attachment_filenames.iter().cloned());
    let search_text = casefold_message_search_text(&values.join("\n"));
    let compact_text = compact_casefolded_message_search_text(&search_text);
    Ok(Some(SearchableLobbyMessage {
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
        code: "invalid_state",
        message: "The canonical message timestamp cannot be indexed.".to_owned(),
    }
}
