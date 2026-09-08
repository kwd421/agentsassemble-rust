use agentsassemble_domain::{
    ChannelHistoryPage, ChannelMessageSend, ROOM_HISTORY_MAX_EVENTS, RoomEvent, RoomHistoryRequest,
    canonical_payload_hash, prepare_channel_message_event,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::Row;

use crate::{
    CommandOutcome, PersistenceError, RoomMutationAuthority, SqliteStore,
    authority::load_active_participant,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    room_channels::require_text_channel,
    room_event_sequence::next_sequence,
    room_turns::support::insert_event,
    room_write_budget::command_size,
};

const ACTION: &str = "channel.message.send";

impl SqliteStore {
    /// Commits one custom-channel message, room event and exact retry result.
    ///
    /// # Errors
    /// Rejects stale authority, removed channels, read-only/muted callers, request
    /// conflicts and storage failures without appending a lobby message or turn input.
    pub async fn execute_channel_message(
        &self,
        authority: RoomMutationAuthority<'_>,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let command = ChannelMessageSend::from_payload(payload).map_err(rejection)?;
        let mut tx = self.pool.begin().await?;
        let principal = authority.resolve(&mut tx).await?;
        let participant =
            load_active_participant(&mut tx, &principal.room_id, &principal.participant_id).await?;
        require_text_channel(&mut tx, &principal.room_id, &command.channel_id).await?;
        let hash = canonical_payload_hash(payload);
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut tx,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            tx.commit().await?;
            return Ok(outcome);
        }
        let event = prepare_channel_message_event(
            &principal,
            &participant,
            &command,
            next_sequence(&mut tx, &principal.room_id).await?,
            Utc::now(),
        )
        .map_err(rejection)?;
        insert_event(&mut tx, &event).await?;
        let result =
            json!({"channel_id": command.channel_id, "event": event, "event_seq": event.seq});
        store_command_result(
            &mut tx,
            (&principal.room_id, &principal.principal_id),
            request_id,
            ACTION,
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(CommandOutcome {
            result,
            event: event.clone(),
            events: vec![event],
            deduplicated: false,
        })
    }

    /// Reads a bounded page from one current registered channel under current room authority.
    ///
    /// # Errors
    /// Rejects stale session/scope, invalid cursors, missing channels and corrupt records.
    pub async fn channel_history_page(
        &self,
        authority: RoomMutationAuthority<'_>,
        channel_id: &str,
        request: RoomHistoryRequest,
    ) -> Result<ChannelHistoryPage, PersistenceError> {
        if request.before_seq < 0 || !(1..=ROOM_HISTORY_MAX_EVENTS).contains(&request.limit) {
            return Err(rejected(
                "bad_request",
                "Channel history cursor or limit is invalid.",
            ));
        }
        let mut tx = self.pool.begin().await?;
        let principal = authority.resolve(&mut tx).await?;
        load_active_participant(&mut tx, &principal.room_id, &principal.participant_id).await?;
        if !principal.capabilities.room_history {
            return Err(rejected(
                "permission_denied",
                "Room history permission is required.",
            ));
        }
        require_text_channel(&mut tx, &principal.room_id, channel_id).await?;
        let last_seq = sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE(MAX(seq), 0) FROM room_events WHERE room_id = ?",
        )
        .bind(&principal.room_id)
        .fetch_one(&mut *tx)
        .await?;
        let before = if request.before_seq == 0 {
            last_seq.saturating_add(1)
        } else {
            request.before_seq
        };
        let rows = sqlx::query("SELECT seq, event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.channel_id') = ? AND json_extract(event_json, '$.type') = 'channel_message_final' AND seq < ? ORDER BY seq DESC LIMIT ?")
            .bind(&principal.room_id).bind(channel_id).bind(before).bind(request.limit + 1)
            .fetch_all(&mut *tx).await?;
        let limit = usize::try_from(request.limit)
            .map_err(|_| rejected("bad_request", "Invalid limit."))?;
        let has_more_before = rows.len() > limit;
        let mut events = Vec::with_capacity(rows.len().min(limit));
        for row in rows.into_iter().take(limit) {
            let event: RoomEvent = serde_json::from_str(row.try_get::<&str, _>("event_json")?)?;
            if event.seq != row.try_get::<i64, _>("seq")? || event.room_id != principal.room_id {
                return Err(rejected(
                    "invalid_state",
                    "Channel event identity is invalid.",
                ));
            }
            events.push(event);
        }
        events.reverse();
        let oldest_seq = events.first().map_or(0, |event| event.seq);
        let room_id = principal.room_id.clone();
        tx.commit().await?;
        Ok(ChannelHistoryPage {
            room_id,
            channel_id: channel_id.to_owned(),
            events,
            oldest_seq,
            last_seq,
            has_more_before,
        })
    }
}

fn rejection(error: agentsassemble_domain::CommandRejection) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: error.code,
        message: error.message,
    }
}

fn rejected(code: &'static str, message: &'static str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "channel_message_tests.rs"]
mod tests;
