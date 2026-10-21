use std::collections::BTreeMap;

use agentsassemble_domain::{
    DurableAgentSession, MAX_MESSAGE_ATTACHMENTS_PER_EVENT, MAX_ROOM_OBSERVATION_AGENT_IDS,
    MAX_ROOM_VIEW_CHARACTERS, QueuedRoomInput, Room, RoomEvent, RoomInputDeliveryKind,
    RoomSettings, has_visible_text, is_provider_input, is_room_observation_view,
    render_persona_context, render_persona_context_parts,
};
use sqlx::{Row, Sqlite, Transaction};

use super::support::{load_event, load_participant, rejected, validate_provider_cursor};
use crate::{
    PersistenceError,
    message_attachments::{
        message_attachment_ids_from_events, message_attachments_from_event,
        message_has_visible_payload, message_visible_text,
    },
    persona_library::selected_persona_card,
};

const MAX_CONTEXT_MESSAGES: usize = 50;

pub(super) struct PreparedRoomInput {
    pub(super) provider_input: String,
    pub(super) session_instructions: Option<String>,
    pub(super) room_view: String,
    pub(super) attachment_ids: Vec<String>,
    pub(super) inflight_inputs: Vec<QueuedRoomInput>,
    pub(super) source_event_id: String,
    pub(super) input_up_to_seq: i64,
    pub(super) room_agent_ids: Vec<String>,
    pub(super) owner_started: bool,
    pub(super) memory: Option<crate::agent_memory::TurnMemoryInput>,
}

pub(super) async fn prepare_room_input(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
    pending_inputs: &[QueuedRoomInput],
    settings: &RoomSettings,
) -> Result<PreparedRoomInput, PersistenceError> {
    let tabletop_tools = settings.tool_mode == "tabletop";
    validate_provider_cursor(transaction, session).await?;
    let roster = load_room_roster(transaction, room, session).await?;
    let pending = load_pending_events(transaction, session, pending_inputs).await?;
    let mut reply_previews = BTreeMap::new();
    for pending in pending.iter().take(MAX_CONTEXT_MESSAGES) {
        load_reply_preview(
            transaction,
            &room.room_id,
            &pending.event,
            &mut reply_previews,
        )
        .await?;
    }
    let inflight = bounded_pending_prefix(room, session, &roster, &pending, &reply_previews)?;
    let Some(source) = inflight.last() else {
        return Err(rejected(
            "queued_room_event_invalid",
            "The ordered-floor assignment has no provider-visible source message.",
        ));
    };
    let context = load_context(
        transaction,
        room,
        session,
        source.event.seq,
        &inflight,
        &roster,
        &mut reply_previews,
    )
    .await?;
    let delivery_kind = inflight[0].input.delivery_kind;
    let attachment_ids = readable_attachment_ids(&inflight, &context)?;
    let room_view = render_room_view(room, session, &roster, &context, &reply_previews)?;
    let persistent = session.public.runtime_kind == "api"
        || matches!(
            session.public.provider_kind.as_str(),
            "codex_live_session" | "claude_code" | "opencode_server"
        );
    let (fixed_persona, persona_context) = if let Some(card) =
        selected_persona_card(transaction, session.public.persona_card_id.as_ref()).await?
    {
        let recent = render_recent_messages(&context)?;
        if persistent {
            render_persona_context_parts(&card, &recent)
        } else {
            (String::new(), render_persona_context(&card, &recent))
        }
    } else {
        (String::new(), String::new())
    };
    let mode = match delivery_kind {
        RoomInputDeliveryKind::OrderedObservation => "Ordered",
        RoomInputDeliveryKind::AmbientObservation => "Ambient",
        RoomInputDeliveryKind::FreeAccessMention => "Free access mention",
    };
    let session_instructions =
        persistent.then(|| render_room_rules(room, session, tabletop_tools, &fixed_persona));
    let mut provider_input = render_observation_input(
        room,
        session,
        mode,
        tabletop_tools,
        &persona_context,
        persistent,
    );
    if delivery_kind == RoomInputDeliveryKind::FreeAccessMention
        || settings.conversation_mode == "free_access"
    {
        provider_input.push_str(&format!(
            "\nCurrent room conversation mode: {}.\n",
            settings.conversation_mode
        ));
        provider_input.push_str(if delivery_kind == RoomInputDeliveryKind::FreeAccessMention {
            "You were called because you were explicitly mentioned. In free_access, nobody prompts you otherwise.\n"
        } else {
            "This turn completes work queued before the mode changed. In free_access, new messages prompt you only when they explicitly mention you.\n"
        });
    }
    if !is_room_observation_view(&room_view)
        || !is_provider_input(&provider_input)
        || !is_provider_input(&format!(
            "{}{}",
            session_instructions.as_deref().unwrap_or_default(),
            provider_input
        ))
    {
        return Err(rejected(
            "provider_turn_input_invalid",
            "The canonical room observation exceeds its provider-visible bound.",
        ));
    }
    // Full access runs without asking only in a turn the agent's owner started:
    // every message this turn answers speaks for the owner.
    let owner = load_participant(transaction, &room.room_id, &session.public.participant_id)
        .await?
        .owner_id;
    let mut owner_started = !owner.is_empty();
    for pending in &inflight {
        if owner_started {
            owner_started = speaks_for_owner(transaction, &owner, &pending.event).await?
                || (session.trusts_ai_handoffs && pending.event.actor.participant_type == "agent");
        }
    }
    // Memory reaches back before the oldest message this turn shows.
    let oldest_shown = context.first().map_or(source.event.seq, |event| event.seq);
    let recent = context
        .iter()
        .rev()
        .take(10)
        .map(message_visible_text)
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    let memory =
        crate::agent_memory::prepare(transaction, &room.room_id, session, oldest_shown, &recent)
            .await?;
    Ok(PreparedRoomInput {
        owner_started,
        memory,
        session_instructions,
        provider_input,
        room_view,
        attachment_ids,
        inflight_inputs: inflight
            .iter()
            .map(|pending| pending.input.clone())
            .collect(),
        source_event_id: source.event.id.clone(),
        input_up_to_seq: source.event.seq,
        room_agent_ids: roster.agent_ids,
    })
}

/// Attachments the turn may read: those of the messages that started it, then those of
/// the other messages shown in its room view, newest first, up to the per-turn limit. A
/// message that only asks about an attachment posted just before it must still reach it.
/// The owner's own message, or one from another of the owner's AIs written
/// in a turn that was itself owner-started. Work handed between the owner's
/// AIs keeps the owner's word; a chain anyone else began never gains it.
/// (An owner who trusts other people's AIs lets any AI's message count.)
pub(super) async fn speaks_for_owner(
    transaction: &mut Transaction<'_, Sqlite>,
    owner: &str,
    event: &RoomEvent,
) -> Result<bool, PersistenceError> {
    let actor = &event.actor.participant_id;
    if actor == owner {
        return Ok(true);
    }
    let text = |key: &str| event.extra.get(key).and_then(serde_json::Value::as_str);
    let (Some(session_id), Some(turn_id)) = (text("session_id"), text("turn_id")) else {
        return Ok(false);
    };
    if event.actor.participant_type != "agent"
        || load_participant(transaction, &event.room_id, actor)
            .await?
            .owner_id
            != owner
    {
        return Ok(false);
    }
    let started = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(json_extract(assignment_json, '$.owner_started'), 0) FROM provider_turn_executions WHERE room_id = ? AND session_id = ? AND participant_id = ? AND turn_id = ?",
    )
    .bind(&event.room_id)
    .bind(session_id)
    .bind(actor)
    .bind(turn_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(started == Some(1))
}

fn readable_attachment_ids(
    inflight: &[&PendingRoomEvent],
    context: &[RoomEvent],
) -> Result<Vec<String>, PersistenceError> {
    let mut ids =
        message_attachment_ids_from_events(inflight.iter().map(|pending| &pending.event))?;
    for id in message_attachment_ids_from_events(context.iter().rev())? {
        if ids.len() >= MAX_MESSAGE_ATTACHMENTS_PER_EVENT {
            break;
        }
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

struct RoomRoster {
    own_role: agentsassemble_domain::ParticipantRole,
    agent_ids: Vec<String>,
    handles: Vec<String>,
    names: Vec<(String, String)>,
    people: Vec<String>,
}

async fn load_room_roster(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    session: &DurableAgentSession,
) -> Result<RoomRoster, PersistenceError> {
    let query_limit = i64::try_from(MAX_ROOM_OBSERVATION_AGENT_IDS + 1).map_err(|_| {
        rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        )
    })?;
    let rows = sqlx::query(
        "SELECT sessions.session_id, json_extract(sessions.session_json, '$.display_name') AS display_name, participants.participant_json FROM agent_sessions AS sessions JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = json_extract(sessions.session_json, '$.participant_id') WHERE sessions.room_id = ? AND sessions.session_id != ? ORDER BY sessions.session_id LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(&session.public.session_id)
    .bind(query_limit)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() > MAX_ROOM_OBSERVATION_AGENT_IDS {
        return Err(rejected(
            "agent_session_capacity",
            "This room exceeds its Agent Session capacity.",
        ));
    }
    let own = load_participant(transaction, &room.room_id, &session.public.participant_id).await?;
    let people = sqlx::query(
        "SELECT participant_id, json_extract(participant_json, '$.display_name') AS display_name FROM participants WHERE room_id = ? AND json_extract(participant_json, '$.participant_type') = 'human' AND json_extract(participant_json, '$.status') = 'joined' ORDER BY participant_id LIMIT ?",
    )
    .bind(&room.room_id)
    .bind(i64::try_from(MAX_ROOM_OBSERVATION_AGENT_IDS).unwrap_or(i64::MAX))
    .fetch_all(&mut **transaction)
    .await?;
    let mut roster = RoomRoster {
        own_role: own.role,
        agent_ids: Vec::new(),
        handles: Vec::new(),
        names: vec![(
            session.public.participant_id.clone(),
            session.public.display_name.clone(),
        )],
        people: people
            .iter()
            .map(|row| {
                let id = row.get::<String, _>("participant_id");
                let name = row.get::<String, _>("display_name");
                render_author(&name, &id)
            })
            .collect(),
    };
    for row in rows {
        let participant = serde_json::from_str::<agentsassemble_domain::Participant>(
            row.get::<&str, _>("participant_json"),
        )?;
        let id = row.get::<String, _>("session_id");
        roster
            .names
            .push((id.clone(), row.get::<String, _>("display_name")));
        if participant.status != agentsassemble_domain::ParticipantStatus::Kicked
            && !participant.muted
        {
            roster.handles.push(format!(
                "{id} (role: {})",
                serde_json::to_string(&participant.role)?
            ));
            roster.agent_ids.push(id);
        }
    }
    Ok(roster)
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
    roster: &RoomRoster,
    pending: &'a [PendingRoomEvent],
    reply_previews: &BTreeMap<String, String>,
) -> Result<Vec<&'a PendingRoomEvent>, PersistenceError> {
    let mut selected = Vec::new();
    let Some(first) = pending.first() else {
        return Err(rejected(
            "queued_room_event_invalid",
            "The room assignment has no provider-visible source message.",
        ));
    };
    for event in pending.iter().take(MAX_CONTEXT_MESSAGES) {
        if event.input.delivery_kind != first.input.delivery_kind
            || (first.input.delivery_kind == RoomInputDeliveryKind::FreeAccessMention
                && !selected.is_empty())
        {
            break;
        }
        selected.push(event);
        // Whole queued messages share the provider observation's attachment budget.
        // Defer the next event intact so its attachments and cursor remain pending.
        if message_attachment_ids_from_events(selected.iter().map(|value| &value.event))?.len()
            > MAX_MESSAGE_ATTACHMENTS_PER_EVENT
        {
            selected.pop();
            break;
        }
        if render_room_view(
            room,
            session,
            roster,
            selected.iter().map(|value| &value.event),
            reply_previews,
        )?
        .chars()
        .count()
            > MAX_ROOM_VIEW_CHARACTERS
        {
            selected.pop();
            break;
        }
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
    mandatory: &[&PendingRoomEvent],
    roster: &RoomRoster,
    reply_previews: &mut BTreeMap<String, String>,
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
        let event: RoomEvent = serde_json::from_str(row.get::<&str, _>("event_json"))?;
        if (!replay_canonical_context
            && event.actor.participant_id == session.public.participant_id)
            || !message_has_visible_payload(&event)?
        {
            continue;
        }
        load_reply_preview(transaction, &room.room_id, &event, reply_previews).await?;
        let seq = event.seq;
        let previous = selected.insert(seq, event);
        if render_room_view(room, session, roster, selected.values(), reply_previews)?
            .chars()
            .count()
            > MAX_ROOM_VIEW_CHARACTERS
        {
            if let Some(previous) = previous {
                selected.insert(seq, previous);
            } else {
                selected.remove(&seq);
            }
        }
    }
    if mandatory
        .iter()
        .any(|pending| !selected.values().any(|event| event.id == pending.event.id))
    {
        return Err(rejected(
            "provider_turn_input_invalid",
            "A queued room message was omitted from the canonical observation.",
        ));
    }
    Ok(selected.into_values().collect())
}

/// Display names are participant-controlled, so a newline must not forge another line.
fn render_author(name: &str, participant_id: &str) -> String {
    format!("{} <@{participant_id}>", name.replace(['\r', '\n'], " "))
}

fn render_room_view<'a>(
    room: &Room,
    session: &DurableAgentSession,
    roster: &RoomRoster,
    context: impl IntoIterator<Item = &'a RoomEvent>,
    reply_previews: &BTreeMap<String, String>,
) -> Result<String, PersistenceError> {
    let mut context = context.into_iter().peekable();
    let mut lines = vec![
        format!("Room: {}", room.label),
        format!(
            "You are: {} [room participant ID: {}; role: {}]",
            session.public.display_name,
            session.public.participant_id,
            serde_json::to_string(&roster.own_role)?
        ),
        format!(
            "Agent handles: {}",
            if roster.handles.is_empty() {
                "none".to_owned()
            } else {
                roster.handles.join(", ")
            }
        ),
        format!(
            "People: {}",
            if roster.people.is_empty() {
                "none".to_owned()
            } else {
                roster.people.join(", ")
            }
        ),
    ];
    let context_floor = if session.public.runtime_kind == "api" {
        session.public.bootstrap_cutoff_seq
    } else {
        session.public.last_provider_sync_seq
    };
    if context
        .peek()
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
            "#{} {} [event {}]: {}",
            event.seq,
            render_author(
                event
                    .display_name
                    .as_deref()
                    .unwrap_or(&event.actor.participant_id),
                &event.actor.participant_id,
            ),
            event.id,
            if has_visible_text(&event_text) {
                event_text.as_str()
            } else {
                "(attachments only)"
            }
        ));
        if super::routing::mentions_target(
            &event.addressed_content(),
            roster
                .names
                .iter()
                .filter(|(id, _)| id != &event.actor.participant_id)
                .map(|(id, name)| (id.as_str(), name.as_str())),
            &session.public.participant_id,
        ) {
            lines.push("  - Mentions you (floor assignment is separate).".to_owned());
        }
        lines.extend(attachments.into_iter().map(|attachment| {
            format!(
                "  - Attachment `{}`: {} ({}; {} bytes)",
                attachment.id, attachment.filename, attachment.content_type, attachment.size
            )
        }));
        if let Some(preview) = reply_previews.get(&event.id) {
            lines.push(preview.clone());
        }
    }
    Ok(lines.join("\n"))
}

// Projection only: resolve current source in the assignment transaction, never store a quote.
async fn load_reply_preview(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event: &RoomEvent,
    previews: &mut BTreeMap<String, String>,
) -> Result<(), PersistenceError> {
    let Some(id) = event
        .extra
        .get("reply_to_event_id")
        .and_then(serde_json::Value::as_str)
    else {
        return Ok(());
    };
    if previews.contains_key(&event.id) {
        return Ok(());
    }
    let source = load_event(tx, room_id, id).await?;
    let preview = if let Some(source) =
        source.filter(|source| source.is_current_lobby_message() && source.seq < event.seq)
    {
        let text = message_visible_text(&source)?;
        let text = if has_visible_text(&text) {
            text
        } else {
            "(attachments only)".to_owned()
        };
        let mut excerpt: String = text.chars().take(200).collect();
        if text.chars().count() > 200 {
            excerpt.push('…');
        }
        let author = render_author(
            source
                .display_name
                .as_deref()
                .unwrap_or(&source.actor.participant_id),
            &source.actor.participant_id,
        );
        format!(
            "  - Reply to event `{id}` by {author}: {} (read_message_context opens the source).",
            serde_json::to_string(&excerpt)?
        )
    } else {
        format!("  - Reply to event `{id}`: source unavailable or deleted.")
    };
    previews.insert(event.id.clone(), preview);
    Ok(())
}

fn render_recent_messages(context: &[RoomEvent]) -> Result<String, PersistenceError> {
    let mut messages = Vec::new();
    for event in context {
        let message_text = message_visible_text(event)?;
        if has_visible_text(&message_text) {
            messages.push(format!(
                "- {}: {message_text}",
                render_author(
                    event
                        .display_name
                        .as_deref()
                        .unwrap_or(&event.actor.participant_id),
                    &event.actor.participant_id,
                )
            ));
        }
    }
    Ok(messages.join("\n"))
}

const ADDRESSING_AND_ROLES: &str = "Reply and call like a Discord user. Set `reply_to_event_id` when it helps to show which message you answer; it shows as a quote. A reply calls that message's author only when your content starts with their `<@ID>`: that mention becomes the reply's notification and is not shown as text. Put `<@ID>` anywhere else only to call someone you are not replying to, and do not open messages with a mention out of habit. A reply that calls nobody does not hand over the floor. Set `next_agent_id` to hand over the floor; you may still use `pass_turn` instead of posting. Your current role is in the room view: a director AI may pin/unpin, create/rename text channels, mute/unmute participants, stop agents, and start conversation-only agents on behalf of its owner within that owner's current authority. Other starts need the owner's confirmation. Director grants no operator rights and cannot assign roles or answer provider approvals.";

fn render_observation_input(
    room: &Room,
    session: &DurableAgentSession,
    mode: &str,
    tabletop_tools: bool,
    persona_context: &str,
    persistent: bool,
) -> String {
    if persistent {
        return if persona_context.is_empty() {
            format!("[{mode} shared-room observation]")
        } else {
            format!(
                "[{mode} shared-room observation]\n\n[Applied bot card or Risu module]\n{persona_context}"
            )
        };
    }
    let mut sections = vec![
        format!("[{mode} shared-room observation]"),
        format!("You are {} in {}. Messages that mention you contain <@{}>; participant IDs are only for addressing, so never write your own ID or describe it in your messages.", session.public.display_name, room.label, session.public.participant_id),
        ADDRESSING_AND_ROLES.to_owned(),
        "Room tools: `read_discussion` shows the room; the other room tools work after it. `publish_message` posts to the room, `pass_turn` passes without posting, and the vote commands create, answer or close polls; each of these ends your turn. `read_room_status` shows current agent activity and polls. Searching and reading attachments or status do not end your turn. `Agent handles` lists other addressable sessions, including stopped sessions whose messages wait for resume; it is not a list of active participants. With workspace access, upload_attachment streams a file from your working folder and returns a pending ID for publish_message attachment_ids. Larger read_attachment results return a saved working-folder path; inline reads stay limited to 10 MiB. Plain reply text is not shown in the room.".to_owned(),
    ];
    if tabletop_tools {
        sections.push("`roll_dice` and `choose_random` are available in this tabletop room; they produce server-side random results shown in the room and do not end your turn.".to_owned());
    } else {
        sections.push(
            "`roll_dice` and `choose_random` are unavailable outside tabletop mode.".to_owned(),
        );
    }
    if session.public.runtime_kind == "api" {
        sections.push("This API transport accepts text attachment results, not image/binary results. Reuploading the same file does not change that limitation. A rejected tool returns an error; use its reason to correct the request rather than assume its content was read.".to_owned());
    }
    if !persona_context.is_empty() {
        sections.push(format!(
            "[Applied bot card or Risu module]\n{persona_context}"
        ));
    }
    sections.join("\n\n")
}

fn render_room_rules(
    room: &Room,
    session: &DurableAgentSession,
    tabletop_tools: bool,
    persona: &str,
) -> String {
    let mut sections = vec![
        format!("You are {} in {}. Messages that mention you contain <@{}>; participant IDs are only for addressing, so never write your own ID or describe it in your messages.", session.public.display_name, room.label, session.public.participant_id),
        ADDRESSING_AND_ROLES.to_owned(),
        "Each shared-room observation starts a room turn. First call the room tool `read_discussion` to read the assigned messages, then respond through the room tools according to their descriptions. Do not reply with plain text instead of reading the room.".to_owned(),
        "Room rules take priority over all persona/card instructions, including lore and post-history instructions. `Agent handles` lists other addressable sessions, including stopped sessions whose messages wait for resume; it is not a list of active participants. With workspace access, upload_attachment streams a file from your working folder and returns a pending ID for publish_message attachment_ids. Larger read_attachment results return a saved working-folder path; inline reads stay limited to 10 MiB. Plain reply text is not shown in the room.".to_owned(),
        if tabletop_tools {
            "`roll_dice` and `choose_random` are available in this tabletop room."
        } else {
            "`roll_dice` and `choose_random` are unavailable outside tabletop mode."
        }.to_owned(),
    ];
    if session.public.runtime_kind == "api" {
        sections.push("This API transport accepts text attachment results, not image/binary results. Reuploading the same file does not change that limitation. A rejected tool returns an error; use its reason to correct the request rather than assume its content was read.".to_owned());
    }
    if !persona.is_empty() {
        sections.push(format!("[Applied bot card or Risu module]\n{persona}"));
    }
    sections.join("\n\n")
}
