use agentsassemble_domain::{RoomChannel, RoomSettings};
use sqlx::{Sqlite, Transaction};

use crate::PersistenceError;

pub(crate) const MESSAGE_CHANNEL_SQL: &str = "CASE json_extract(events.event_json, '$.type') WHEN 'channel_message_final' THEN json_extract(events.event_json, '$.channel_id') ELSE 'lobby' END";

pub(crate) async fn require_message_channel(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
) -> Result<(), PersistenceError> {
    if channel_id == "lobby" {
        return Ok(());
    }
    require_text_channel(tx, room_id, channel_id).await
}

pub(crate) async fn require_text_channel(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    channel_id: &str,
) -> Result<(), PersistenceError> {
    let encoded =
        sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_one(&mut **tx)
            .await?;
    let settings: RoomSettings = serde_json::from_str(&encoded)?;
    match settings
        .channels
        .iter()
        .find(|channel| channel.id == channel_id)
    {
        Some(channel) if channel.channel_type == "text" => Ok(()),
        Some(_) => Err(rejected(
            "channel_unavailable",
            "This channel is not a text channel.",
        )),
        None => Err(rejected(
            "channel_not_found",
            "This text channel does not exist.",
        )),
    }
}

pub(crate) async fn transition_channels(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    current: &[RoomChannel],
    next: &[RoomChannel],
) -> Result<(), PersistenceError> {
    if current == next {
        return Ok(());
    }
    for channel in next {
        if let Some(previous) = current.iter().find(|previous| previous.id == channel.id) {
            if previous.channel_type != channel.channel_type || previous.created_at != channel.created_at {
                return Err(rejected("channel_identity_conflict", "Channel type and creation identity cannot change."));
            }
        } else if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM room_channel_retirements WHERE room_id = ? AND channel_id = ?)")
            .bind(room_id).bind(&channel.id).fetch_one(&mut **tx).await? {
            return Err(rejected("channel_retired", "A removed channel id cannot be reused."));
        }
    }
    for removed in current
        .iter()
        .filter(|old| !next.iter().any(|new| new.id == old.id))
    {
        sqlx::query("INSERT INTO room_channel_retirements(room_id, channel_id) VALUES (?, ?)")
            .bind(room_id)
            .bind(&removed.id)
            .execute(&mut **tx)
            .await?;
        // A generic room-history read must not expose a removed channel's text.
        // Retain sequence/actor identity while retiring its visible message state.
        for table in ["room_message_pins", "room_message_search_records"] {
            let query = format!(
                "DELETE FROM {table} WHERE room_id = ? AND event_seq IN (SELECT seq FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.channel_id') = ? AND json_extract(event_json, '$.type') = 'channel_message_final')"
            );
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(room_id)
                .bind(room_id)
                .bind(&removed.id)
                .execute(&mut **tx)
                .await?;
        }
        sqlx::query("UPDATE room_events SET event_json = json_set(event_json, '$.content', '', '$.message_deleted', json('true')) WHERE room_id = ? AND json_extract(event_json, '$.channel_id') = ? AND json_extract(event_json, '$.type') = 'channel_message_final'")
            .bind(room_id).bind(&removed.id).execute(&mut **tx).await?;
    }
    Ok(())
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
