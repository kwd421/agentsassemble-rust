use std::collections::{BTreeMap, HashSet};

use agentsassemble_domain::{
    DurableAgentSession, Room, RoomEvent, clean_message, has_visible_text,
};
use sqlx::{Row, Sqlite, Transaction};

use super::support::{load_event, rejected, validate_provider_cursor};
use crate::PersistenceError;

const MAX_CONTEXT_MESSAGES: usize = 50;
const MAX_ROOM_VIEW_CHARS: usize = 20_000;

pub(super) struct PreparedRoomInput {
    pub(super) provider_input: String,
    pub(super) room_view: String,
    pub(super) inflight_event_ids: Vec<String>,
    pub(super) source_event_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) room_agent_ids: Vec<String>,
}

pub(super) async fn prepare_room_input(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    pending_event_ids: &[String],
) -> Result<PreparedRoomInput, PersistenceError> {
    validate_provider_cursor(transaction, session).await?;
    let room_agent_ids = load_room_agent_ids(transaction, room, session).await?;
    let pending = load_pending_events(transaction, session, pending_event_ids).await?;
    let inflight = bounded_pending_prefix(room, session, &room_agent_ids, &pending)?;
    let Some(source) = inflight.last() else {
        return Err(rejected(
            "queued_room_event_invalid",
            "The ordered-floor assignment has no provider-visible source message.",
        ));
    };
    let mandatory_ids = inflight
        .iter()
        .map(|event| event.id.as_str())
        .collect::<HashSet<_>>();
    let context = load_context(
        transaction,
        room,
        session,
        source.seq,
        &mandatory_ids,
        &inflight,
        &room_agent_ids,
    )
    .await?;
    let room_view = render_room_view(room, session, &room_agent_ids, &context);
    if room_view.chars().count() > MAX_ROOM_VIEW_CHARS {
        return Err(rejected(
            "provider_turn_input_invalid",
            "The canonical room observation exceeds its provider-visible bound.",
        ));
    }
    Ok(PreparedRoomInput {
        provider_input: render_provider_input(room, session),
        room_view,
        inflight_event_ids: inflight.iter().map(|event| event.id.clone()).collect(),
        source_event_id: source.id.clone(),
        input_up_to_seq: source.seq,
        room_agent_ids,
    })
}

async fn load_room_agent_ids(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
) -> Result<Vec<String>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT sessions.session_id, participants.participant_json FROM agent_sessions AS sessions JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = json_extract(sessions.session_json, '$.participant_id') WHERE sessions.room_id = ? AND sessions.session_id != ? ORDER BY sessions.session_id LIMIT 65",
    )
    .bind(&room.room_id)
    .bind(&session.public.session_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > 64 {
        return Err(rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        ));
    }
    let mut agent_ids = Vec::new();
    for row in rows {
        let participant = serde_json::from_str::<agentsassemble_domain::Participant>(
            row.get::<String, _>("participant_json").as_str(),
        )?;
        if participant.status != agentsassemble_domain::ParticipantStatus::Kicked
            && !participant.muted
        {
            agent_ids.push(row.get::<String, _>("session_id"));
        }
    }
    Ok(agent_ids)
}

async fn load_pending_events(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
    pending_event_ids: &[String],
) -> Result<Vec<RoomEvent>, PersistenceError> {
    let mut events = Vec::with_capacity(pending_event_ids.len());
    let mut previous_seq = session.public.last_provider_sync_seq;
    for event_id in pending_event_ids {
        let event = load_event(transaction, &session.public.room_id, event_id)
            .await?
            .ok_or_else(|| rejected("room_event_missing", "Queued room input is missing."))?;
        if event.event_type != "message_final"
            || event.actor.participant_id == session.public.participant_id
            || event.seq <= previous_seq
            || !event.content.as_deref().is_some_and(has_visible_text)
        {
            return Err(rejected(
                "queued_room_event_invalid",
                "Queued room input does not match ordered-floor authority.",
            ));
        }
        previous_seq = event.seq;
        events.push(event);
    }
    Ok(events)
}

fn bounded_pending_prefix<'a>(
    room: &Room,
    session: &DurableAgentSession,
    room_agent_ids: &[String],
    pending: &'a [RoomEvent],
) -> Result<Vec<&'a RoomEvent>, PersistenceError> {
    let mut selected = Vec::new();
    for event in pending.iter().take(MAX_CONTEXT_MESSAGES) {
        let mut candidate = selected.clone();
        candidate.push(event);
        let candidate_events = candidate
            .iter()
            .map(|value| (*value).clone())
            .collect::<Vec<_>>();
        if render_room_view(room, session, room_agent_ids, &candidate_events)
            .chars()
            .count()
            > MAX_ROOM_VIEW_CHARS
        {
            break;
        }
        selected = candidate;
    }
    if selected.is_empty() {
        return Err(rejected(
            "provider_turn_input_invalid",
            "The oldest queued room message cannot fit the canonical observation bound.",
        ));
    }
    Ok(selected)
}

async fn load_context(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    up_to_seq: i64,
    mandatory_ids: &HashSet<&str>,
    mandatory: &[&RoomEvent],
    room_agent_ids: &[String],
) -> Result<Vec<RoomEvent>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? AND seq <= ? AND json_extract(event_json, '$.type') = 'message_final' ORDER BY seq DESC LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(session.public.last_provider_sync_seq)
    .bind(up_to_seq)
    .bind(i64::try_from(MAX_CONTEXT_MESSAGES).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    let mut selected = BTreeMap::<i64, RoomEvent>::new();
    for event in mandatory {
        selected.insert(event.seq, (*event).clone());
    }
    for row in rows {
        if selected.len() >= MAX_CONTEXT_MESSAGES {
            break;
        }
        let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
        if event.actor.participant_id == session.public.participant_id
            || !event.content.as_deref().is_some_and(has_visible_text)
        {
            continue;
        }
        let mut candidate = selected.clone();
        candidate.insert(event.seq, event);
        let values = candidate.values().cloned().collect::<Vec<_>>();
        if render_room_view(room, session, room_agent_ids, &values)
            .chars()
            .count()
            <= MAX_ROOM_VIEW_CHARS
        {
            selected = candidate;
        }
    }
    if mandatory_ids
        .iter()
        .any(|event_id| !selected.values().any(|event| event.id == *event_id))
    {
        return Err(rejected(
            "provider_turn_input_invalid",
            "A queued room message was omitted from the canonical observation.",
        ));
    }
    Ok(selected.into_values().collect())
}

fn render_room_view(
    room: &Room,
    session: &DurableAgentSession,
    room_agent_ids: &[String],
    context: &[RoomEvent],
) -> String {
    let mut lines = vec![
        format!("Room: {}", room.label),
        format!("You are: {}", session.public.display_name),
        format!(
            "Agent handles: {}",
            if room_agent_ids.is_empty() {
                "none".to_owned()
            } else {
                room_agent_ids.join(", ")
            }
        ),
    ];
    if context
        .first()
        .is_some_and(|event| event.seq > session.public.last_provider_sync_seq.saturating_add(1))
    {
        lines.push("[Earlier room updates are outside this bounded observation.]".to_owned());
    }
    lines.extend(context.iter().filter_map(|event| {
        let event_text = clean_message(event.content.as_deref().unwrap_or_default(), 12_000);
        has_visible_text(&event_text).then(|| {
            format!(
                "#{} {}: {}",
                event.seq,
                event
                    .display_name
                    .as_deref()
                    .unwrap_or(&event.actor.participant_id),
                event_text
            )
        })
    }));
    lines.join("\n")
}

fn render_provider_input(room: &Room, session: &DurableAgentSession) -> String {
    [
        "[Ordered shared-room observation]".to_owned(),
        format!("Your room identity is {} in {}.", session.public.display_name, room.label),
        "Call `read_discussion` before deciding. Then finish with exactly one `publish_message` for a substantive reply or `decline_to_speak` when you have nothing useful to add. Ordinary assistant final text is not a room publication.".to_owned(),
    ]
    .join("\n\n")
}
