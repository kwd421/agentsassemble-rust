use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, DurableAgentSession, Participant, Room, RoomEvent, RoomSettings, RoomStatus,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{PersistenceError, turn_authority::active_turn_authority};

const MAX_PROVIDER_TURN_ID_BYTES: usize = 128;

async fn provider_cursor_is_valid(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<bool, PersistenceError> {
    if session.public.last_provider_sync_seq == 0 {
        return Ok(session.public.last_provider_sync_event_id.is_empty());
    }
    let event = load_event(
        transaction,
        &session.public.room_id,
        &session.public.last_provider_sync_event_id,
    )
    .await?;
    Ok(event.is_some_and(|event| event.seq == session.public.last_provider_sync_seq))
}

pub(super) async fn validate_provider_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    if provider_cursor_is_valid(transaction, session).await? {
        Ok(())
    } else {
        Err(rejected(
            "provider_sync_cursor_mismatch",
            "The provider sync cursor is outside canonical room history.",
        ))
    }
}

pub(super) async fn validate_input_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let event = load_event(
        transaction,
        &session.public.room_id,
        &session.input_up_to_event_id,
    )
    .await?;
    if session.input_up_to_seq <= 0
        || event.is_none_or(|event| event.seq != session.input_up_to_seq)
    {
        return Err(rejected(
            "provider_sync_cursor_mismatch",
            "The active provider input cursor is outside canonical room history.",
        ));
    }
    Ok(())
}

pub(crate) async fn load_active_room(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<(Room, RoomSettings), PersistenceError> {
    let row = sqlx::query("SELECT room_json, settings_json FROM rooms WHERE room_id = ?")
        .bind(room_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(PersistenceError::RoomMissing)?;
    let room: Room = serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
    if room.status != RoomStatus::Active {
        return Err(rejected(
            "room_inactive",
            "Closed or archived rooms do not accept commands.",
        ));
    }
    let settings = serde_json::from_str(row.get::<String, _>("settings_json").as_str())?;
    Ok((room, settings))
}

pub(crate) async fn load_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PersistenceError::ParticipantMissing)?;
    Ok(serde_json::from_str(&value)?)
}

pub(super) async fn load_event(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
) -> Result<Option<RoomEvent>, PersistenceError> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.id') = ? LIMIT 1",
    )
    .bind(room_id)
    .bind(event_id)
    .fetch_optional(&mut **transaction)
    .await?;
    value
        .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
        .transpose()
}

pub(crate) async fn next_sequence(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<i64, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?)
}

pub(crate) async fn insert_event(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
        .bind(&event.room_id)
        .bind(event.seq)
        .bind(serde_json::to_string(event)?)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn internal_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    event_type: &str,
    actor_is_agent: bool,
    content: Option<String>,
    extra: BTreeMap<String, Value>,
) -> Result<RoomEvent, PersistenceError> {
    let participant_id = if actor_is_agent {
        session.public.participant_id.clone()
    } else {
        "room-system".to_owned()
    };
    let participant_type = if actor_is_agent { "agent" } else { "system" };
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &session.public.room_id).await?,
        created_at: Utc::now(),
        room_id: session.public.room_id.clone(),
        event_type: event_type.to_owned(),
        actor: Actor {
            participant_id: participant_id.clone(),
            participant_type: participant_type.to_owned(),
        },
        participant_id: Some(session.public.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(participant_id),
        actor_type: Some(participant_type.to_owned()),
        display_name: Some(session.public.display_name.clone()),
        content,
        message_kind: (event_type == "message_final").then(|| "message".to_owned()),
        extra,
    };
    insert_event(transaction, &event).await?;
    Ok(event)
}

pub(super) async fn turn_started_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "turn_started",
        false,
        None,
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(session.public.active_turn_id)),
            (
                "source_event_id".to_owned(),
                json!(session.active_source_event_id),
            ),
            (
                "provider_context_up_to_seq".to_owned(),
                json!(session.input_up_to_seq),
            ),
        ]),
    )
    .await
}

pub(super) async fn turn_state_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "turn_state",
        false,
        None,
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(session.public.active_turn_id)),
            ("phase".to_owned(), json!(session.public.turn_phase)),
        ]),
    )
    .await
}

pub(super) async fn session_state_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "agent_session_state",
        false,
        None,
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            (
                "runtime_status".to_owned(),
                json!(session.public.runtime_status),
            ),
            ("agent_session".to_owned(), json!(session.public())),
        ]),
    )
    .await
}

pub(super) async fn agent_final_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    provider_turn_id: &str,
    source_event_id: &str,
    content: String,
    target_agent_id: &str,
) -> Result<RoomEvent, PersistenceError> {
    session.inflight_inputs.first().ok_or_else(|| {
        rejected(
            "stored_turn_authority_invalid",
            "Active provider turn has no canonical input provenance.",
        )
    })?;
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &session.public.room_id).await?,
        created_at: Utc::now(),
        room_id: session.public.room_id.clone(),
        event_type: "message_final".to_owned(),
        actor: Actor {
            participant_id: session.public.participant_id.clone(),
            participant_type: "agent".to_owned(),
        },
        participant_id: Some(session.public.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(session.public.participant_id.clone()),
        actor_type: Some("agent".to_owned()),
        display_name: Some(session.public.display_name.clone()),
        content: Some(content),
        message_kind: Some("message".to_owned()),
        extra: BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(turn_id)),
            ("provider_turn_id".to_owned(), json!(provider_turn_id)),
            ("source_event_id".to_owned(), json!(source_event_id)),
            ("target_agent_id".to_owned(), json!(target_agent_id)),
            ("message_source".to_owned(), json!("room_portal")),
        ]),
    };
    insert_event(transaction, &event).await?;
    Ok(event)
}

pub(super) async fn turn_finished_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    status: &str,
    provider_turn_id: Option<&str>,
    reason_code: Option<&str>,
) -> Result<RoomEvent, PersistenceError> {
    let mut extra = BTreeMap::from([
        ("session_id".to_owned(), json!(session.public.session_id)),
        ("turn_id".to_owned(), json!(turn_id)),
        ("status".to_owned(), json!(status)),
    ]);
    if let Some(provider_turn_id) = provider_turn_id {
        extra.insert("provider_turn_id".to_owned(), json!(provider_turn_id));
    }
    if let Some(reason_code) = reason_code {
        extra.insert("reason_code".to_owned(), json!(reason_code));
    }
    internal_event(transaction, session, "turn_finished", false, None, extra).await
}

pub(super) async fn error_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    error_code: &str,
    message: &str,
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "error",
        false,
        Some(message.to_owned()),
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(turn_id)),
            ("error_code".to_owned(), json!(error_code)),
        ]),
    )
    .await
}

pub(super) fn require_active_turn(
    session: &DurableAgentSession,
    turn_id: &str,
) -> Result<(), PersistenceError> {
    if turn_id.is_empty()
        || turn_id != session.public.active_turn_id
        || !turn_authority_is_active(session)?
    {
        return Err(rejected(
            "stale_provider_turn",
            "Provider turn completion does not match durable room authority.",
        ));
    }
    Ok(())
}

fn turn_authority_is_active(session: &DurableAgentSession) -> Result<bool, PersistenceError> {
    active_turn_authority(session).map_err(|_| {
        rejected(
            "stored_turn_authority_invalid",
            "Stored Agent Session turn authority is inconsistent.",
        )
    })
}

pub(super) fn clear_active_turn_fields(session: &mut DurableAgentSession) {
    session.active_source_event_id.clear();
    session.input_up_to_event_id.clear();
    session.input_up_to_seq = 0;
}

pub(super) fn validate_identifier(value: &str, code: &'static str) -> Result<(), PersistenceError> {
    if value.is_empty()
        || value.len() > MAX_PROVIDER_TURN_ID_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(rejected(code, "Provider turn identity is invalid."));
    }
    Ok(())
}

pub(super) fn public_error_code(value: &str) -> &'static str {
    match value {
        "provider_turn_cancelled" => "provider_turn_cancelled",
        "provider_turn_timeout" => "provider_turn_timeout",
        "provider_model_mismatch" => "provider_model_mismatch",
        "provider_runtime_exited" => "provider_runtime_exited",
        "provider_runtime_restart_required" => "provider_runtime_restart_required",
        "room_observation_unconfirmed" => "room_observation_unconfirmed",
        "room_portal_publication_missing" => "room_portal_publication_missing",
        "provider_protocol_invalid" | "provider_protocol_mismatch" => "provider_protocol_invalid",
        _ => "provider_turn_failed",
    }
}

pub(super) fn rejection(error: agentsassemble_domain::CommandRejection) -> PersistenceError {
    rejected(error.code, error.message)
}

pub(super) fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
