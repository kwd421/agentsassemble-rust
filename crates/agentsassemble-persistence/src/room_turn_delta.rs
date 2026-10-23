//! What an API agent has not seen since its last completed turn. A harness
//! that still holds the conversation where it saw the earlier messages sends
//! only these, so every turn extends the same cached prompt instead of
//! replacing its end with a fresh window.
use std::collections::BTreeMap;

use agentsassemble_domain::{DurableAgentSession, MAX_ROOM_VIEW_CHARACTERS, Room, RoomEvent};
use sqlx::{Row, Sqlite, Transaction};

use super::{
    MAX_CONTEXT_MESSAGES, PendingRoomEvent, RoomRoster, load_reply_preview, render_room_view_from,
};
use crate::{
    PersistenceError, message_attachments::message_has_visible_payload, room_turns::RoomViewDelta,
};

const CONTINUATION: &str = "[Only the room messages since your last turn; the earlier ones are above in this conversation.]";

/// `None` when there is no completed turn to continue from, or when more
/// arrived than one view holds; the harness then shows the full window.
pub(super) async fn load_view_delta(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    up_to_seq: i64,
    mandatory: &[&PendingRoomEvent],
    roster: &RoomRoster,
    reply_previews: &mut BTreeMap<String, String>,
) -> Result<Option<RoomViewDelta>, PersistenceError> {
    let since_seq = session.public.last_provider_sync_seq;
    if session.public.runtime_kind != "api" || since_seq <= 0 || since_seq >= up_to_seq {
        return Ok(None);
    }
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? AND seq <= ? AND json_extract(event_json, '$.type') = 'message_final' ORDER BY seq LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(since_seq)
    .bind(up_to_seq)
    .bind(i64::try_from(MAX_CONTEXT_MESSAGES + 1).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > MAX_CONTEXT_MESSAGES {
        return Ok(None);
    }
    let mut events = Vec::new();
    for row in rows {
        let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
        // The agent's own messages are already in its conversation as the
        // tool calls that posted them.
        if event.actor.participant_id == session.public.participant_id
            || !message_has_visible_payload(&event)?
        {
            continue;
        }
        load_reply_preview(transaction, &room.room_id, &event, reply_previews).await?;
        events.push(event);
    }
    if mandatory
        .iter()
        .any(|pending| !events.iter().any(|event| event.id == pending.event.id))
    {
        return Ok(None);
    }
    // Everything before the first message shown is in the conversation, so the
    // view never claims earlier updates are missing.
    let floor = events.first().map_or(since_seq, |event| event.seq - 1);
    let view = format!(
        "{CONTINUATION}\n{}",
        render_room_view_from(room, session, roster, &events, reply_previews, floor)?
    );
    Ok((view.chars().count() <= MAX_ROOM_VIEW_CHARACTERS)
        .then_some(RoomViewDelta { since_seq, view }))
}
