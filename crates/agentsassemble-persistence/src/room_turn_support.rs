use std::collections::{BTreeMap, HashMap, HashSet};

use agentsassemble_domain::{
    Actor, DurableAgentSession, Participant, ParticipantStatus, Room, RoomEvent, RoomSettings,
    RoomStatus, clean_message, has_visible_text,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::routing::{last_direct_target, sampled_candidate_indexes};
use super::{AgentTurnAssignment, PreparedAssignment};
use crate::{
    PersistenceError,
    agent_lifecycle::{load_session, save_session},
    turn_queue::{MAX_QUEUED_EVENT_IDS, event_id_queue_is_canonical},
};

const MAX_PROVIDER_INPUT_CHARS: usize = 20_000;
const MAX_CONTEXT_MESSAGES: usize = 50;
const MAX_PROVIDER_TURN_ID_BYTES: usize = 128;

pub(super) async fn queue_ordered_message(
    transaction: &mut Transaction<'_, Sqlite>,
    settings: &RoomSettings,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    if settings.conversation_mode != "ordered" || event.event_type != "message_final" {
        return Ok(());
    }
    let mut candidates = eligible_route_sessions(transaction, event).await?;
    if candidates.is_empty() {
        return Ok(());
    }
    let content = event.content.as_deref().unwrap_or_default();
    let direct = last_direct_target(content, candidates.iter().map(|(session, _)| session));
    let selected = if let Some(direct) = direct {
        direct
    } else {
        let (message_counts, previous_speaker) =
            recent_agent_speaking_state(transaction, &event.room_id, event.seq).await?;
        if settings.ordered_exclude_previous_speaker && candidates.len() > 1 {
            candidates.retain(|(session, _)| session.public.session_id != previous_speaker);
        }
        candidates.sort_by(|left, right| left.0.public.session_id.cmp(&right.0.public.session_id));
        let sampled = sampled_candidate_indexes(candidates.len());
        sampled
            .into_iter()
            .map(|index| &candidates[index].0)
            .min_by_key(|session| {
                message_counts
                    .get(&session.public.session_id)
                    .copied()
                    .unwrap_or(0)
            })
            .map(|session| session.public.session_id.clone())
            .ok_or_else(|| rejected("ordered_floor_empty", "No ordered floor target remained."))?
    };
    let mut session = load_session(transaction, &event.room_id, &selected).await?;
    if !session.pending_event_ids.iter().any(|id| id == &event.id) {
        let queued = session
            .inflight_event_ids
            .len()
            .saturating_add(session.pending_event_ids.len());
        if queued >= MAX_QUEUED_EVENT_IDS {
            return Err(rejected(
                "ordered_floor_queue_full",
                "The selected Agent Session ordered-floor queue is full.",
            ));
        }
        session.pending_event_ids.push(event.id.clone());
        session.public.updated_at = Utc::now();
        save_session(transaction, &session).await?;
    }
    Ok(())
}

pub(super) async fn assign_oldest_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    settings: &RoomSettings,
) -> Result<Option<PreparedAssignment>, PersistenceError> {
    if settings.conversation_mode != "ordered"
        || room_has_active_turn(transaction, &room.room_id).await?
    {
        return Ok(None);
    }
    let rows = sqlx::query(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? ORDER BY session_id LIMIT 65",
    )
    .bind(&room.room_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > 64 {
        return Err(rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        ));
    }
    let mut candidates = Vec::new();
    for row in rows {
        let mut session: DurableAgentSession =
            serde_json::from_str(row.get::<String, _>("session_json").as_str())?;
        let _ = turn_authority_is_active(&session)?;
        if !session_is_assignable(&session) {
            continue;
        }
        let participant =
            load_participant(transaction, &room.room_id, &session.public.participant_id).await?;
        if participant.status != ParticipantStatus::Joined || participant.muted {
            continue;
        }
        let valid = valid_pending_ids(transaction, &session).await?;
        if valid != session.pending_event_ids {
            session.pending_event_ids.clone_from(&valid);
            save_session(transaction, &session).await?;
        }
        let Some(first) = valid.first() else {
            continue;
        };
        let sequence = event_sequence(transaction, &room.room_id, first).await?;
        candidates.push((sequence, session));
    }
    candidates.sort_by(|left, right| {
        (left.0, left.1.public.session_id.as_str())
            .cmp(&(right.0, right.1.public.session_id.as_str()))
    });
    let Some((_, mut session)) = candidates.into_iter().next() else {
        return Ok(None);
    };
    let inflight = session.pending_event_ids.clone();
    let source_event_id = inflight.last().cloned().unwrap_or_default();
    let input_up_to_seq = event_sequence(transaction, &room.room_id, &source_event_id).await?;
    let provider_input = build_provider_input(transaction, room, &session, input_up_to_seq).await?;
    let turn_id = format!("turn-{}", &Uuid::new_v4().simple().to_string()[..12]);
    "busy".clone_into(&mut session.public.runtime_status);
    "thinking".clone_into(&mut session.public.turn_phase);
    session.public.active_turn_id.clone_from(&turn_id);
    session.inflight_event_ids = inflight;
    session.pending_event_ids.clear();
    session.active_source_event_id.clone_from(&source_event_id);
    session.input_up_to_event_id.clone_from(&source_event_id);
    session.input_up_to_seq = input_up_to_seq;
    session.public.updated_at = Utc::now();
    save_session(transaction, &session).await?;
    let started = turn_started_event(transaction, &session).await?;
    let state = turn_state_event(transaction, &session).await?;
    let session_event = session_state_event(transaction, &session).await?;
    Ok(Some(PreparedAssignment {
        assignment: AgentTurnAssignment {
            session,
            turn_id,
            provider_input,
        },
        events: vec![started, state, session_event],
    }))
}

async fn eligible_route_sessions(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<Vec<(DurableAgentSession, Participant)>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? ORDER BY session_id LIMIT 65",
    )
    .bind(&event.room_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > 64 {
        return Err(rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        ));
    }
    let actor_id = &event.actor.participant_id;
    let mut sessions = Vec::new();
    for row in rows {
        let session: DurableAgentSession =
            serde_json::from_str(row.get::<String, _>("session_json").as_str())?;
        let _ = turn_authority_is_active(&session)?;
        if session.public.participant_id == *actor_id
            || !session.public.enabled
            || session.public.status != "attached"
            || !matches!(session.public.runtime_status.as_str(), "idle" | "busy")
            || !session.public.provider_session_active
            || !session.lifecycle_intent_action.is_empty()
        {
            continue;
        }
        validate_provider_cursor(transaction, &session).await?;
        let participant =
            load_participant(transaction, &event.room_id, &session.public.participant_id).await?;
        if participant.status == ParticipantStatus::Joined && !participant.muted {
            sessions.push((session, participant));
        }
    }
    Ok(sessions)
}

async fn recent_agent_speaking_state(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    before_seq: i64,
) -> Result<(HashMap<String, u64>, String), PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq < ? ORDER BY seq DESC LIMIT 100",
    )
    .bind(room_id)
    .bind(before_seq)
    .fetch_all(&mut **transaction)
    .await?;
    let mut counts = HashMap::new();
    let mut previous = String::new();
    for row in rows {
        let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
        if event.event_type != "message_final" || event.actor.participant_type != "agent" {
            continue;
        }
        if previous.is_empty() {
            previous.clone_from(&event.actor.participant_id);
        }
        *counts.entry(event.actor.participant_id).or_insert(0) += 1;
    }
    Ok((counts, previous))
}

async fn build_provider_input(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    up_to_seq: i64,
) -> Result<String, PersistenceError> {
    validate_provider_cursor(transaction, session).await?;
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? AND seq <= ? ORDER BY seq DESC LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(session.public.last_provider_sync_seq)
    .bind(up_to_seq)
    .bind(i64::try_from(MAX_CONTEXT_MESSAGES).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    let mut messages = rows
        .into_iter()
        .map(|row| serde_json::from_str::<RoomEvent>(row.get::<String, _>("event_json").as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    messages.retain(|event| event.event_type == "message_final");
    messages.reverse();
    let mut context = messages
        .iter()
        .filter_map(|event| {
            let content = event.content.as_deref()?;
            has_visible_text(content).then(|| {
                format!(
                    "#{} {}: {}",
                    event.seq,
                    event
                        .display_name
                        .as_deref()
                        .unwrap_or(&event.actor.participant_id),
                    clean_message(content, 12_000)
                )
            })
        })
        .collect::<Vec<_>>();
    let bootstrap = session.public.turn_count == 0;
    loop {
        let input = render_provider_input(room, session, &context, bootstrap);
        if input.chars().count() <= MAX_PROVIDER_INPUT_CHARS {
            return Ok(input);
        }
        if context.is_empty() {
            return Err(rejected(
                "provider_turn_input_invalid",
                "The bounded room context could not fit the provider turn input.",
            ));
        }
        context.remove(0);
    }
}

fn render_provider_input(
    room: &Room,
    session: &DurableAgentSession,
    context: &[String],
    bootstrap: bool,
) -> String {
    let mut parts = Vec::new();
    if bootstrap {
        parts.extend([
            "[Agent Session bootstrap]".to_owned(),
            "You are participating in a shared AgentsAssemble room.".to_owned(),
            "Do not reveal runtime secrets or hidden chain-of-thought.".to_owned(),
        ]);
    }
    parts.extend([
        "[Your room identity]".to_owned(),
        format!("Your display name in this room is: {}", session.public.display_name),
        format!("The room name is: {}", room.label),
        "[Room update since your last turn]".to_owned(),
        context.join("\n"),
        "[Your turn]".to_owned(),
        "Answer the latest room message once. Return only the room-visible response as your provider final. Do not call room publish/read tools; this context is already canonical.".to_owned(),
    ]);
    parts.join("\n\n").trim().to_owned()
}

async fn valid_pending_ids(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<Vec<String>, PersistenceError> {
    let mut seen = HashSet::new();
    let mut valid = Vec::new();
    for event_id in &session.pending_event_ids {
        if event_id.is_empty() || !seen.insert(event_id.clone()) {
            continue;
        }
        let event = load_event(transaction, &session.public.room_id, event_id)
            .await?
            .ok_or_else(|| rejected("room_event_missing", "Queued room input is missing."))?;
        if event.event_type != "message_final"
            || event.actor.participant_id == session.public.participant_id
        {
            return Err(rejected(
                "queued_room_event_invalid",
                "Queued room input does not match ordered-floor authority.",
            ));
        }
        if event.seq > session.public.last_provider_sync_seq {
            valid.push(event_id.clone());
        }
    }
    Ok(valid)
}

fn session_is_assignable(session: &DurableAgentSession) -> bool {
    session.public.enabled
        && session.public.status == "attached"
        && session.public.runtime_status == "idle"
        && session.public.provider_session_active
        && session.public.active_turn_id.is_empty()
        && session.inflight_event_ids.is_empty()
        && session.lifecycle_intent_action.is_empty()
}

fn turn_authority_is_active(session: &DurableAgentSession) -> Result<bool, PersistenceError> {
    if !event_id_queue_is_canonical(
        session
            .inflight_event_ids
            .iter()
            .chain(&session.pending_event_ids),
    ) {
        return Err(rejected(
            "stored_turn_authority_invalid",
            "Stored Agent Session turn queue authority is inconsistent or oversized.",
        ));
    }
    let active = !session.public.active_turn_id.is_empty()
        && session.public.enabled
        && session.public.status == "attached"
        && session.public.runtime_status == "busy"
        && matches!(session.public.turn_phase.as_str(), "thinking" | "streaming")
        && !session.inflight_event_ids.is_empty()
        && session.inflight_event_ids.last() == Some(&session.active_source_event_id)
        && session.active_source_event_id == session.input_up_to_event_id
        && session.input_up_to_seq > 0;
    let clear = session.public.active_turn_id.is_empty()
        && session.public.turn_phase.is_empty()
        && session.inflight_event_ids.is_empty()
        && session.active_source_event_id.is_empty()
        && session.input_up_to_event_id.is_empty()
        && session.input_up_to_seq == 0;
    match (active, clear) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err(rejected(
            "stored_turn_authority_invalid",
            "Stored Agent Session turn authority is inconsistent.",
        )),
    }
}

async fn room_has_active_turn(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<bool, PersistenceError> {
    let rows = sqlx::query("SELECT session_json FROM agent_sessions WHERE room_id = ? LIMIT 65")
        .bind(room_id)
        .fetch_all(&mut **transaction)
        .await?;
    let mut active_count = 0_u8;
    for row in rows {
        let session: DurableAgentSession =
            serde_json::from_str(row.get::<String, _>("session_json").as_str())?;
        if turn_authority_is_active(&session)? {
            active_count = active_count.saturating_add(1);
        }
    }
    if active_count > 1 {
        return Err(rejected(
            "stored_turn_authority_invalid",
            "More than one Agent Session owns the ordered room turn.",
        ));
    }
    Ok(active_count == 1)
}

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

async fn validate_provider_cursor(
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

pub(super) async fn load_active_room(
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

pub(super) async fn load_participant(
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

async fn load_event(
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

async fn event_sequence(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
) -> Result<i64, PersistenceError> {
    load_event(transaction, room_id, event_id)
        .await?
        .map(|event| event.seq)
        .ok_or_else(|| rejected("room_event_missing", "Queued room input is missing."))
}

pub(super) async fn next_sequence(
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

pub(super) async fn insert_event(
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
        relay_depth: (event_type == "message_final").then_some(0),
        extra,
    };
    insert_event(transaction, &event).await?;
    Ok(event)
}

async fn turn_started_event(
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

async fn turn_state_event(
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
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "message_final",
        true,
        Some(content),
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(turn_id)),
            ("provider_turn_id".to_owned(), json!(provider_turn_id)),
            ("source_event_id".to_owned(), json!(source_event_id)),
            ("message_source".to_owned(), json!("provider_final")),
        ]),
    )
    .await
}

pub(super) async fn turn_finished_event(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    turn_id: &str,
    status: &str,
) -> Result<RoomEvent, PersistenceError> {
    internal_event(
        transaction,
        session,
        "turn_finished",
        false,
        None,
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("turn_id".to_owned(), json!(turn_id)),
            ("status".to_owned(), json!(status)),
        ]),
    )
    .await
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
