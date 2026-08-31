use std::collections::{BTreeMap, HashSet};

use agentsassemble_domain::{
    DurableAgentSession, QueuedRoomInput, Room, RoomEvent, RoomInputDeliveryKind, has_visible_text,
    render_persona_context,
};
use sqlx::{Row, Sqlite, Transaction};

use super::support::{load_event, rejected, validate_provider_cursor};
use crate::{
    PersistenceError,
    message_attachments::{
        message_attachment_ids_from_events, message_attachments_from_event,
        message_has_visible_payload, message_visible_text,
    },
    persona_library::selected_persona_card,
};

const MAX_CONTEXT_MESSAGES: usize = 50;
const MAX_ROOM_VIEW_CHARS: usize = 20_000;

pub(super) struct PreparedRoomInput {
    pub(super) provider_input: String,
    pub(super) room_view: String,
    pub(super) attachment_ids: Vec<String>,
    pub(super) inflight_inputs: Vec<QueuedRoomInput>,
    pub(super) source_event_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) room_agent_ids: Vec<String>,
}

pub(super) async fn prepare_room_input(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    pending_inputs: &[QueuedRoomInput],
    tabletop_tools: bool,
) -> Result<PreparedRoomInput, PersistenceError> {
    validate_provider_cursor(transaction, session).await?;
    let room_agent_ids = load_room_agent_ids(transaction, room, session).await?;
    let pending = load_pending_events(transaction, session, pending_inputs).await?;
    let inflight = bounded_pending_prefix(room, session, &room_agent_ids, &pending)?;
    let Some(source) = inflight.last() else {
        return Err(rejected(
            "queued_room_event_invalid",
            "The ordered-floor assignment has no provider-visible source message.",
        ));
    };
    let mandatory_ids = inflight
        .iter()
        .map(|pending| pending.event.id.as_str())
        .collect::<HashSet<_>>();
    let context = load_context(
        transaction,
        room,
        session,
        source.event.seq,
        &mandatory_ids,
        &inflight,
        &room_agent_ids,
    )
    .await?;
    let delivery_kind = inflight[0].input.delivery_kind;
    let attachment_ids =
        message_attachment_ids_from_events(inflight.iter().map(|pending| &pending.event))?;
    let rendered_context = render_room_view(room, session, &room_agent_ids, &context)?;
    let persona_context = if let Some(card) =
        selected_persona_card(transaction, session.public.persona_card_id.as_ref()).await?
    {
        render_persona_context(&card, &render_recent_messages(&context)?)
    } else {
        String::new()
    };
    let (provider_input, room_view) = match delivery_kind {
        RoomInputDeliveryKind::OrderedObservation => (
            render_observation_input(room, session, "Ordered", tabletop_tools, &persona_context),
            rendered_context,
        ),
        RoomInputDeliveryKind::AmbientObservation => (
            render_observation_input(room, session, "Ambient", tabletop_tools, &persona_context),
            rendered_context,
        ),
    };
    if room_view.chars().count() > MAX_ROOM_VIEW_CHARS
        || provider_input.chars().count() > MAX_ROOM_VIEW_CHARS
    {
        return Err(rejected(
            "provider_turn_input_invalid",
            "The canonical room observation exceeds its provider-visible bound.",
        ));
    }
    Ok(PreparedRoomInput {
        provider_input,
        room_view,
        attachment_ids,
        inflight_inputs: inflight
            .iter()
            .map(|pending| pending.input.clone())
            .collect(),
        source_event_id: source.event.id.clone(),
        input_up_to_seq: source.event.seq,
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
    pending_inputs: &[QueuedRoomInput],
) -> Result<Vec<PendingRoomEvent>, PersistenceError> {
    let mut events = Vec::with_capacity(pending_inputs.len());
    let mut previous_seq = session.public.last_provider_sync_seq;
    for input in pending_inputs {
        let event = load_event(transaction, &session.public.room_id, &input.event_id)
            .await?
            .ok_or_else(|| rejected("room_event_missing", "Queued room input is missing."))?;
        if event.event_type != "message_final"
            || event.actor.participant_id == session.public.participant_id
            || event.seq <= previous_seq
            || !message_has_visible_payload(&event)?
        {
            return Err(rejected(
                "queued_room_event_invalid",
                "Queued room input does not match ordered-floor authority.",
            ));
        }
        previous_seq = event.seq;
        events.push(PendingRoomEvent {
            input: input.clone(),
            event,
        });
    }
    Ok(events)
}

struct PendingRoomEvent {
    input: QueuedRoomInput,
    event: RoomEvent,
}

fn bounded_pending_prefix<'a>(
    room: &Room,
    session: &DurableAgentSession,
    room_agent_ids: &[String],
    pending: &'a [PendingRoomEvent],
) -> Result<Vec<&'a PendingRoomEvent>, PersistenceError> {
    let mut selected = Vec::new();
    let Some(first) = pending.first() else {
        return Err(rejected(
            "queued_room_event_invalid",
            "The room assignment has no provider-visible source message.",
        ));
    };
    for event in pending.iter().take(MAX_CONTEXT_MESSAGES) {
        if event.input.delivery_kind != first.input.delivery_kind {
            break;
        }
        let mut candidate = selected.clone();
        candidate.push(event);
        let candidate_events = candidate
            .iter()
            .map(|value| value.event.clone())
            .collect::<Vec<_>>();
        if render_room_view(room, session, room_agent_ids, &candidate_events)?
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
    mandatory: &[&PendingRoomEvent],
    room_agent_ids: &[String],
) -> Result<Vec<RoomEvent>, PersistenceError> {
    let replay_canonical_context = session.public.runtime_kind == "api";
    let context_floor = if replay_canonical_context {
        session.public.bootstrap_cutoff_seq
    } else {
        session.public.last_provider_sync_seq
    };
    let rows = sqlx::query(
        "SELECT event_json FROM room_events WHERE room_id = ? AND seq > ? AND seq <= ? AND json_extract(event_json, '$.type') = 'message_final' ORDER BY seq DESC LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(context_floor)
    .bind(up_to_seq)
    .bind(i64::try_from(MAX_CONTEXT_MESSAGES).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    let mut selected = BTreeMap::<i64, RoomEvent>::new();
    for pending in mandatory {
        selected.insert(pending.event.seq, pending.event.clone());
    }
    for row in rows {
        if selected.len() >= MAX_CONTEXT_MESSAGES {
            break;
        }
        let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
        if (!replay_canonical_context
            && event.actor.participant_id == session.public.participant_id)
            || !message_has_visible_payload(&event)?
        {
            continue;
        }
        let mut candidate = selected.clone();
        candidate.insert(event.seq, event);
        let values = candidate.values().cloned().collect::<Vec<_>>();
        if render_room_view(room, session, room_agent_ids, &values)?
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
) -> Result<String, PersistenceError> {
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
    let context_floor = if session.public.runtime_kind == "api" {
        session.public.bootstrap_cutoff_seq
    } else {
        session.public.last_provider_sync_seq
    };
    if context
        .first()
        .is_some_and(|event| event.seq > context_floor.saturating_add(1))
    {
        lines.push("[Earlier room updates are outside this bounded observation.]".to_owned());
    }
    for event in context {
        let event_text = message_visible_text(event)?;
        let attachments = message_attachments_from_event(event)?;
        if !has_visible_text(&event_text) && attachments.is_empty() {
            continue;
        }
        lines.push(format!(
            "#{} {}: {}",
            event.seq,
            event
                .display_name
                .as_deref()
                .unwrap_or(&event.actor.participant_id),
            if has_visible_text(&event_text) {
                event_text.as_str()
            } else {
                "(attachments only)"
            }
        ));
        lines.extend(attachments.into_iter().map(|attachment| {
            format!(
                "  - Attachment `{}`: {} ({}; {} bytes)",
                attachment.id, attachment.filename, attachment.content_type, attachment.size
            )
        }));
    }
    Ok(lines.join("\n"))
}

fn render_recent_messages(context: &[RoomEvent]) -> Result<String, PersistenceError> {
    let mut messages = Vec::new();
    for event in context {
        let message_text = message_visible_text(event)?;
        if has_visible_text(&message_text) {
            messages.push(format!(
                "- {}: {message_text}",
                event
                    .display_name
                    .as_deref()
                    .unwrap_or(&event.actor.participant_id)
            ));
        }
    }
    Ok(messages.join("\n"))
}

fn render_observation_input(
    room: &Room,
    session: &DurableAgentSession,
    mode: &str,
    tabletop_tools: bool,
    persona_context: &str,
) -> String {
    let mut sections = vec![
        format!("[{mode} shared-room observation]"),
        format!("Your room identity is {} in {}.", session.public.display_name, room.label),
        "Call `read_discussion` before deciding. Then finish with exactly one terminal action exposed by the room transport, choosing the action that fulfills the current room request. Ordinary assistant final text is not a room publication.".to_owned(),
    ];
    if tabletop_tools {
        sections.push(if session.public.provider_kind == "antigravity_live_session" {
            "Official game randomness is available through the exact bound room helper described by the transport instruction; do not invent a result yourself.".to_owned()
        } else {
            "For official game randomness, use the `roll_dice` or `choose_random` room tool; do not invent a result yourself.".to_owned()
        });
    }
    if !persona_context.is_empty() {
        sections.push(format!(
            "[Applied bot card or Risu module]\n{persona_context}"
        ));
    }
    sections.join("\n\n")
}
