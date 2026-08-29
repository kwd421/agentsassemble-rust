use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, MAX_LOBBY_MESSAGE_PINS, RoomEvent,
    has_visible_text, is_message_pin_event_id,
};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    human_session_authority::revalidate_human_session,
    room_user_identity::{require_current_local_room_manager, resolve_room_user_identity},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedLobbyMessage {
    pub event_id: String,
    pub pinned_at: String,
    pub seq: i64,
    pub author: String,
    pub content: String,
    pub created_at: String,
    pub attachment_filenames: Vec<String>,
}

impl SqliteStore {
    /// Lists lobby pins while the canonical local operator remains this room's manager.
    ///
    /// # Errors
    ///
    /// Fails when local authority, a stored pointer, its event, or persistence is invalid.
    pub async fn local_lobby_message_pins(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_local_operator(&mut transaction, room_id, user_id, participant_id).await?;
        let pins = load_pins(&mut transaction, room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Lists lobby pins while an exact durable human session retains room-history permission.
    ///
    /// # Errors
    ///
    /// Fails when session authority, permission, a stored pointer, its event, or persistence is
    /// invalid.
    pub async fn human_session_lobby_message_pins(
        &self,
        expected: &HumanSessionAuthorization,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_permission(
            principal.capabilities.room_history,
            "This room session cannot read message history.",
        )?;
        let pins = load_pins(&mut transaction, &principal.room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Pins or unpins one lobby message as the canonical local operator.
    ///
    /// # Errors
    ///
    /// Fails without writing when local authority or the target message is invalid.
    pub async fn set_local_lobby_message_pin(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        event_id: &str,
        pinned: bool,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_local_operator(&mut transaction, room_id, user_id, participant_id).await?;
        set_pin(&mut transaction, room_id, event_id, pinned, Utc::now()).await?;
        let pins = load_pins(&mut transaction, room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Pins or unpins one lobby message while an exact durable human session remains writable.
    ///
    /// # Errors
    ///
    /// Fails without writing when session authority, permission, or the target message is invalid.
    pub async fn set_human_session_lobby_message_pin(
        &self,
        expected: &HumanSessionAuthorization,
        event_id: &str,
        pinned: bool,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_permission(
            principal.capabilities.message_modify,
            "This room session cannot modify messages.",
        )?;
        set_pin(
            &mut transaction,
            &principal.room_id,
            event_id,
            pinned,
            Utc::now(),
        )
        .await?;
        let pins = load_pins(&mut transaction, &principal.room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }
}

async fn authorize_local_operator(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    user_id: &str,
    participant_id: &str,
) -> Result<(), PersistenceError> {
    if user_id != LOCAL_OPERATOR_USER_ID || participant_id != LOCAL_OPERATOR_PARTICIPANT_ID {
        return Err(rejected(
            "permission_denied",
            "Only the local room operator may use local pin authority.",
        ));
    }
    let identity =
        resolve_room_user_identity(transaction, room_id, user_id, participant_id).await?;
    require_current_local_room_manager(transaction, &identity).await?;
    Ok(())
}

fn require_permission(allowed: bool, message: &'static str) -> Result<(), PersistenceError> {
    if allowed {
        Ok(())
    } else {
        Err(rejected("permission_denied", message))
    }
}

async fn set_pin(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
    pinned: bool,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    validate_event_id(event_id)?;
    let event = load_target_message(transaction, room_id, event_id).await?;
    if !pinned {
        sqlx::query("DELETE FROM room_message_pins WHERE room_id = ? AND event_id = ?")
            .bind(room_id)
            .bind(event_id)
            .execute(&mut **transaction)
            .await?;
        return Ok(());
    }
    ensure_pin_capacity(transaction, room_id, event_id).await?;
    sqlx::query(
        "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES (?, ?, ?, ?) ON CONFLICT(room_id, event_id) DO UPDATE SET event_seq = excluded.event_seq, pinned_at = excluded.pinned_at",
    )
    .bind(room_id)
    .bind(event_id)
    .bind(event.seq)
    .bind(now.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn ensure_pin_capacity(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
) -> Result<(), PersistenceError> {
    let other_pins = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_message_pins WHERE room_id = ? AND event_id != ?",
    )
    .bind(room_id)
    .bind(event_id)
    .fetch_one(&mut **transaction)
    .await?;
    if other_pins >= MAX_LOBBY_MESSAGE_PINS {
        return Err(rejected(
            "pin_limit_reached",
            "This room has reached the message pin limit.",
        ));
    }
    Ok(())
}

async fn load_target_message(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
) -> Result<RoomEvent, PersistenceError> {
    let rows = sqlx::query(
        "SELECT seq, event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.id') = ? LIMIT 2",
    )
    .bind(room_id)
    .bind(event_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() != 1 {
        return Err(if rows.is_empty() {
            rejected("message_missing", "The message was not found.")
        } else {
            invalid_state("Stored room event identity is not unique.")
        });
    }
    let row = &rows[0];
    let seq = row.get::<i64, _>("seq");
    let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
    require_message_event(&event, room_id, event_id, seq)?;
    Ok(event)
}

async fn load_pins(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT pins.event_id, pins.event_seq, pins.pinned_at, events.event_json FROM room_message_pins AS pins JOIN room_events AS events ON events.room_id = pins.room_id AND events.seq = pins.event_seq WHERE pins.room_id = ? ORDER BY pins.pinned_at DESC, pins.event_id ASC LIMIT ?",
    )
    .bind(room_id)
    .bind(MAX_LOBBY_MESSAGE_PINS + 1)
    .fetch_all(&mut **transaction)
    .await?;
    if i64::try_from(rows.len())
        .map_err(|_| invalid_state("Stored message pin count is invalid."))?
        > MAX_LOBBY_MESSAGE_PINS
    {
        return Err(invalid_state("Stored message pin count exceeds its limit."));
    }
    rows.into_iter()
        .map(|row| {
            let event_id = row.get::<String, _>("event_id");
            let event_seq = row.get::<i64, _>("event_seq");
            let pinned_at = row.get::<i64, _>("pinned_at");
            let event: RoomEvent =
                serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
            require_message_event(&event, room_id, &event_id, event_seq)?;
            project_pin(event, pinned_at)
        })
        .collect()
}

fn project_pin(
    event: RoomEvent,
    pinned_at_micros: i64,
) -> Result<PinnedLobbyMessage, PersistenceError> {
    let pinned_at = DateTime::from_timestamp_micros(pinned_at_micros)
        .ok_or_else(|| invalid_state("Stored message pin timestamp is invalid."))?;
    let author = event
        .display_name
        .filter(|name| !name.is_empty())
        .or_else(|| (!event.actor.participant_id.is_empty()).then_some(event.actor.participant_id))
        .unwrap_or_else(|| "Room".to_owned());
    let content = event
        .content
        .ok_or_else(|| invalid_state("Stored message pin target has no content."))?;
    Ok(PinnedLobbyMessage {
        event_id: event.id,
        pinned_at: pinned_at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        seq: event.seq,
        author,
        content,
        created_at: event
            .created_at
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
        attachment_filenames: Vec::new(),
    })
}

fn require_message_event(
    event: &RoomEvent,
    room_id: &str,
    event_id: &str,
    seq: i64,
) -> Result<(), PersistenceError> {
    if event.room_id != room_id || event.id != event_id || event.seq != seq {
        return Err(invalid_state("Stored message pin target is inconsistent."));
    }
    if event.event_type != "message_final"
        || event.extra.get("message_deleted") == Some(&Value::Bool(true))
    {
        return Err(rejected("message_missing", "The message was not found."));
    }
    if !event.content.as_deref().is_some_and(has_visible_text) {
        return Err(invalid_state(
            "Stored message pin target has invalid content.",
        ));
    }
    Ok(())
}

fn validate_event_id(event_id: &str) -> Result<(), PersistenceError> {
    if !is_message_pin_event_id(event_id) {
        return Err(rejected("bad_request", "event_id is invalid."));
    }
    Ok(())
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

fn invalid_state(message: impl Into<String>) -> PersistenceError {
    rejected("invalid_state", message)
}

#[cfg(test)]
#[path = "message_pin_tests.rs"]
mod tests;
