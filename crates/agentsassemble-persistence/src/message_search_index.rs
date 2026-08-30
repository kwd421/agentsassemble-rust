use agentsassemble_domain::{
    MAX_MESSAGE_SEARCH_AUTHOR_CHARACTERS, MAX_MESSAGE_SEARCH_CONTENT_CHARACTERS, RoomEvent,
    casefold_message_search_text, clean_message_search_value,
    compact_casefolded_message_search_text, room_event_is_owner_only,
};
use serde_json::Value;
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, message_attachments::message_attachments_from_event};

pub(crate) async fn index_lobby_message(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    if !event.is_current_lobby_message() || room_event_is_owner_only(event) {
        return Ok(());
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
    let attachments = message_attachments_from_event(event)?;
    let mut values = Vec::with_capacity(attachments.len() + 2);
    values.push(author);
    if !content.is_empty() {
        values.push(content);
    }
    values.extend(
        attachments
            .into_iter()
            .map(|attachment| attachment.filename),
    );
    let search_text = casefold_message_search_text(&values.join("\n"));
    let compact_text = compact_casefolded_message_search_text(&search_text);
    let created_at_nanos = event
        .created_at
        .timestamp_nanos_opt()
        .filter(|value| *value > 0)
        .ok_or_else(invalid_search_event)?;
    sqlx::query(
        "INSERT INTO room_message_search_records(\
            room_id, event_seq, event_id, created_at_nanos, search_text, compact_text\
         ) VALUES (?, ?, ?, ?, ?, ?)",
    )
    .bind(&event.room_id)
    .bind(event.seq)
    .bind(&event.id)
    .bind(created_at_nanos)
    .bind(search_text)
    .bind(compact_text)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn invalid_search_event() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: "The canonical message timestamp cannot be indexed.".to_owned(),
    }
}
