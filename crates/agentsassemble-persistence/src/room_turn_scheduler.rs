use std::collections::{HashMap, HashSet};

use agentsassemble_domain::{
    DurableAgentSession, Participant, ParticipantRole, ParticipantStatus, QueuedRoomInput, Room,
    RoomEvent, RoomInputDeliveryKind, RoomSettings, has_visible_text,
};
use chrono::Utc;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use super::context::prepare_room_input;
use super::routing::{last_direct_target, sampled_candidate_indexes};
use super::support::{
    load_event, load_participant, rejected, session_state_event, turn_started_event,
    turn_state_event, validate_provider_cursor,
};
use super::{AgentTurnAssignment, PreparedAssignment};
use crate::{
    PersistenceError,
    agent_lifecycle::{load_session, save_session},
    turn_authority::active_turn_authority,
    turn_queue::MAX_QUEUED_EVENT_IDS,
};

/// Routes one canonical public message into the current room-mode queues.
pub(super) async fn route_message(
    transaction: &mut Transaction<'_, Sqlite>,
    settings: &RoomSettings,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    if !is_routable_message(event) {
        return Ok(());
    }
    let sessions = route_sessions(transaction, event).await?;
    let targets = match settings.conversation_mode.as_str() {
        "ordered" => ordered_targets(transaction, settings, event, &sessions).await?,
        "ambient" => ambient_targets(event, &sessions),
        _ => {
            return Err(rejected(
                "stored_room_settings_invalid",
                "Stored room conversation mode is invalid.",
            ));
        }
    };
    for (session_id, delivery_kind) in targets {
        queue_input(transaction, event, &session_id, delivery_kind).await?;
    }
    Ok(())
}

/// Assigns every currently available session allowed by the current floor mode.
pub(super) async fn assign_available_pending(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    settings: &RoomSettings,
) -> Result<Vec<PreparedAssignment>, PersistenceError> {
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
    let mut any_active = false;
    let mut candidates = Vec::new();
    for row in rows {
        let session: DurableAgentSession =
            serde_json::from_str(row.get::<String, _>("session_json").as_str())?;
        any_active |= turn_authority_is_active(&session)?;
        if !session_is_assignable(&session) {
            continue;
        }
        let participant =
            load_participant(transaction, &room.room_id, &session.public.participant_id).await?;
        if participant.status != ParticipantStatus::Joined || participant.muted {
            continue;
        }
        let valid = valid_pending_inputs(transaction, &session).await?;
        let Some(first) = valid.first() else {
            continue;
        };
        let sequence = event_sequence(transaction, &room.room_id, &first.event_id).await?;
        candidates.push((sequence, session));
    }
    if settings.conversation_mode == "ordered" && any_active {
        return Ok(Vec::new());
    }
    candidates.sort_by(|left, right| {
        (left.0, left.1.public.session_id.as_str())
            .cmp(&(right.0, right.1.public.session_id.as_str()))
    });
    if settings.conversation_mode == "ordered" {
        candidates.truncate(1);
    }
    let mut prepared = Vec::with_capacity(candidates.len());
    for (_, session) in candidates {
        prepared.push(prepare_assignment(transaction, room, settings, session).await?);
    }
    Ok(prepared)
}

fn is_routable_message(event: &RoomEvent) -> bool {
    event.event_type == "message_final"
        && event.extra.get("message_source").and_then(Value::as_str) != Some("room_tool_result")
        && !event
            .message_kind
            .as_deref()
            .is_some_and(|kind| matches!(kind, "vote_cast" | "vote_withdraw" | "vote_close"))
        && event.content.as_deref().is_some_and(has_visible_text)
}

async fn ordered_targets(
    transaction: &mut Transaction<'_, Sqlite>,
    settings: &RoomSettings,
    event: &RoomEvent,
    sessions: &[(DurableAgentSession, Participant)],
) -> Result<Vec<(String, RoomInputDeliveryKind)>, PersistenceError> {
    let content = event.content.as_deref().unwrap_or_default();
    let structured_target = event
        .extra
        .get("target_agent_id")
        .and_then(Value::as_str)
        .filter(|target| {
            sessions.iter().any(|(session, _)| {
                session.public.session_id == *target && !is_actor(session, event)
            })
        })
        .map(str::to_owned);
    // The structured handoff is the earliest direct target. A later explicit
    // mention in the message body owns the floor, matching the product's
    // final-mention rule for model recaps followed by a next-speaker call.
    let direct = last_direct_target(
        content,
        sessions
            .iter()
            .filter(|(session, _)| !is_actor(session, event))
            .map(|(session, _)| session),
    )
    .or(structured_target);
    let selected = if let Some(direct) = direct {
        let participant = sessions
            .iter()
            .find(|(session, _)| session.public.session_id == direct)
            .map(|(_, participant)| participant)
            .ok_or_else(|| {
                rejected("ordered_floor_empty", "The direct floor target is missing.")
            })?;
        if participant.status == ParticipantStatus::Kicked || participant.muted {
            return Ok(Vec::new());
        }
        direct
    } else {
        let mut candidates = sessions
            .iter()
            .filter(|(session, participant)| {
                !is_actor(session, event) && route_session_is_eligible(session, participant)
            })
            .collect::<Vec<_>>();
        if event.actor.participant_type == "agent" {
            let actor =
                load_participant(transaction, &event.room_id, &event.actor.participant_id).await?;
            if actor.role != ParticipantRole::Director {
                let directors = candidates
                    .iter()
                    .copied()
                    .filter(|(_, participant)| participant.role == ParticipantRole::Director)
                    .collect::<Vec<_>>();
                if !directors.is_empty() {
                    candidates = directors;
                }
            }
        }
        if candidates.is_empty() {
            return Ok(Vec::new());
        }
        let (message_counts, previous_speaker) =
            recent_agent_speaking_state(transaction, &event.room_id, event.seq).await?;
        if settings.ordered_exclude_previous_speaker && candidates.len() > 1 {
            candidates.retain(|(session, _)| session.public.session_id != previous_speaker);
        }
        candidates.sort_by(|left, right| left.0.public.session_id.cmp(&right.0.public.session_id));
        sampled_candidate_indexes(candidates.len())
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
    Ok(vec![(selected, RoomInputDeliveryKind::OrderedObservation)])
}

fn ambient_targets(
    event: &RoomEvent,
    sessions: &[(DurableAgentSession, Participant)],
) -> Vec<(String, RoomInputDeliveryKind)> {
    sessions
        .iter()
        .filter(|(session, participant)| {
            !is_actor(session, event) && route_session_is_eligible(session, participant)
        })
        .map(|(session, _)| {
            (
                session.public.session_id.clone(),
                RoomInputDeliveryKind::AmbientObservation,
            )
        })
        .collect()
}

async fn queue_input(
    transaction: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
    session_id: &str,
    delivery_kind: RoomInputDeliveryKind,
) -> Result<(), PersistenceError> {
    let mut session = load_session(transaction, &event.room_id, session_id).await?;
    if session
        .pending_inputs
        .iter()
        .chain(&session.inflight_inputs)
        .any(|input| input.event_id == event.id)
    {
        return Ok(());
    }
    let queued = session
        .pending_inputs
        .len()
        .saturating_add(session.inflight_inputs.len());
    if queued >= MAX_QUEUED_EVENT_IDS {
        let (code, message) = if delivery_kind == RoomInputDeliveryKind::OrderedObservation {
            (
                "ordered_floor_queue_full",
                "The selected Agent Session ordered-floor queue is full.",
            )
        } else {
            (
                "room_turn_queue_full",
                "The selected Agent Session room-input queue is full.",
            )
        };
        return Err(rejected(code, message));
    }
    session.pending_inputs.push(QueuedRoomInput {
        event_id: event.id.clone(),
        delivery_kind,
    });
    session.public.updated_at = Utc::now();
    save_session(transaction, &session).await
}

async fn route_sessions(
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
    let mut sessions = Vec::with_capacity(rows.len());
    for row in rows {
        let session: DurableAgentSession =
            serde_json::from_str(row.get::<String, _>("session_json").as_str())?;
        let _ = turn_authority_is_active(&session)?;
        validate_provider_cursor(transaction, &session).await?;
        let participant =
            load_participant(transaction, &event.room_id, &session.public.participant_id).await?;
        sessions.push((session, participant));
    }
    Ok(sessions)
}

fn is_actor(session: &DurableAgentSession, event: &RoomEvent) -> bool {
    session.public.participant_id == event.actor.participant_id
        || session.public.session_id == event.actor.participant_id
}

fn route_session_is_eligible(session: &DurableAgentSession, participant: &Participant) -> bool {
    participant.status == ParticipantStatus::Joined
        && !participant.muted
        && session.public.enabled
        && session.public.status == "attached"
        && matches!(session.public.runtime_status.as_str(), "idle" | "busy")
        && session.public.provider_session_active
        && session.lifecycle_intent_action.is_empty()
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

async fn valid_pending_inputs(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<Vec<QueuedRoomInput>, PersistenceError> {
    let mut seen = HashSet::new();
    let mut previous_seq = session.public.last_provider_sync_seq;
    let mut valid = Vec::with_capacity(session.pending_inputs.len());
    for input in &session.pending_inputs {
        if input.event_id.is_empty() || !seen.insert(&input.event_id) {
            return Err(rejected(
                "stored_turn_authority_invalid",
                "Stored Agent Session turn queue authority is inconsistent.",
            ));
        }
        let event = load_event(transaction, &session.public.room_id, &input.event_id)
            .await?
            .ok_or_else(|| rejected("room_event_missing", "Queued room input is missing."))?;
        if event.event_type != "message_final"
            || event.actor.participant_id == session.public.participant_id
            || event.seq <= previous_seq
            || !event.content.as_deref().is_some_and(has_visible_text)
        {
            return Err(rejected(
                "queued_room_event_invalid",
                "Queued room input does not match canonical room turn authority.",
            ));
        }
        previous_seq = event.seq;
        valid.push(input.clone());
    }
    Ok(valid)
}

fn session_is_assignable(session: &DurableAgentSession) -> bool {
    session.public.enabled
        && session.public.status == "attached"
        && session.public.runtime_status == "idle"
        && session.public.provider_session_active
        && session.public.active_turn_id.is_empty()
        && session.inflight_inputs.is_empty()
        && session.lifecycle_intent_action.is_empty()
}

fn turn_authority_is_active(session: &DurableAgentSession) -> Result<bool, PersistenceError> {
    active_turn_authority(session).map_err(|_| {
        rejected(
            "stored_turn_authority_invalid",
            "Stored Agent Session turn authority is inconsistent.",
        )
    })
}

async fn prepare_assignment(
    transaction: &mut Transaction<'_, Sqlite>,
    room: &Room,
    settings: &RoomSettings,
    mut session: DurableAgentSession,
) -> Result<PreparedAssignment, PersistenceError> {
    let prepared_input = prepare_room_input(
        transaction,
        room,
        &session,
        &session.pending_inputs,
        settings.tool_mode == "tabletop",
    )
    .await?;
    let inflight = prepared_input.inflight_inputs;
    let delivery_kind = inflight[0].delivery_kind;
    let source_event_id = prepared_input.source_event_id;
    let input_up_to_seq = prepared_input.input_up_to_seq;
    let turn_id = format!("turn-{}", &Uuid::new_v4().simple().to_string()[..12]);
    "busy".clone_into(&mut session.public.runtime_status);
    "thinking".clone_into(&mut session.public.turn_phase);
    session.public.active_turn_id.clone_from(&turn_id);
    session.inflight_inputs.clone_from(&inflight);
    session.pending_inputs.drain(..inflight.len());
    session.active_source_event_id.clone_from(&source_event_id);
    session.input_up_to_event_id.clone_from(&source_event_id);
    session.input_up_to_seq = input_up_to_seq;
    session.public.updated_at = Utc::now();
    save_session(transaction, &session).await?;
    let started = turn_started_event(transaction, &session).await?;
    let state = turn_state_event(transaction, &session).await?;
    let session_event = session_state_event(transaction, &session).await?;
    Ok(PreparedAssignment {
        assignment: AgentTurnAssignment {
            session,
            turn_id,
            delivery_kind,
            provider_input: prepared_input.provider_input,
            room_view: prepared_input.room_view,
            room_agent_ids: prepared_input.room_agent_ids,
            tabletop_tools: settings.tool_mode == "tabletop",
        },
        events: vec![started, state, session_event],
    })
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
