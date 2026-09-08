use crate::participant_rows::load_participant_by_key as load_optional_participant;
use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AgentLifecycleAction, AgentLifecycleIntentStatus, AgentRuntimeCustody,
    AgentRuntimeStatus, AgentSession, AgentSessionDraft, AgentSessionStatus,
    AuthenticatedPrincipal, DurableAgentSession, Participant, ParticipantRole, ParticipantStatus,
    RoomEvent,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError,
    agent_session_rows::{load_optional_agent_session_row, update_agent_session_row},
    persona_library::resolve_persona_selection,
    room_event_sequence::next_sequence,
    sqlite::MAX_AGENT_SESSIONS_PER_ROOM,
};

pub(crate) struct AgentCreationRecords {
    pub session: DurableAgentSession,
    pub result: Value,
    pub committed_events: Vec<RoomEvent>,
}

pub(crate) async fn create_or_reuse_agent_records(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    draft: &AgentSessionDraft,
    start_operation_id: Option<&str>,
    allow_exact_reuse: bool,
) -> Result<AgentCreationRecords, PersistenceError> {
    let existing_session =
        load_optional_agent_session_row(transaction, &principal.room_id, &draft.agent_id).await?;
    let existing_participant =
        load_optional_participant(transaction, &principal.room_id, &draft.agent_id).await?;
    match (existing_session, existing_participant) {
        (None, None) => {
            create_agent_records(transaction, principal, draft, start_operation_id).await
        }
        (Some(mut session), Some(participant))
            if allow_exact_reuse
                && session.public.process_ownership == "server"
                && session.runtime_profile_key == draft.runtime_profile_key
                && participant.owner_id == principal.participant_id =>
        {
            if !session.lifecycle_intent_action.is_none()
                || !session.lifecycle_intent_id.is_empty()
                || !session.lifecycle_intent_status.is_none()
            {
                return Err(rejected(
                    "operation_in_progress",
                    "Another provider lifecycle operation is still in progress.",
                ));
            }
            if let Some(operation_id) = start_operation_id {
                prepare_start(&mut session, operation_id);
                save_session(transaction, &session).await?;
            }
            let result = base_result(&session.public, &participant, &[]);
            Ok(AgentCreationRecords {
                session,
                result,
                committed_events: Vec::new(),
            })
        }
        _ => Err(rejected(
            "session_exists",
            "An Agent Session with this identity already exists.",
        )),
    }
}

async fn create_agent_records(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    draft: &AgentSessionDraft,
    start_operation_id: Option<&str>,
) -> Result<AgentCreationRecords, PersistenceError> {
    let session_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM agent_sessions WHERE room_id = ?")
            .bind(&principal.room_id)
            .fetch_one(&mut **transaction)
            .await?;
    if session_count >= MAX_AGENT_SESSIONS_PER_ROOM {
        return Err(rejected(
            "agent_session_capacity",
            "This room has reached its Agent Session capacity.",
        ));
    }
    let now = Utc::now();
    let participant = Participant {
        room_id: principal.room_id.clone(),
        participant_id: draft.agent_id.clone(),
        display_name: draft.display_name.clone(),
        avatar_image_url: String::new(),
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Detached,
        role: ParticipantRole::Agent,
        owner_id: principal.participant_id.clone(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    let (last_message_id, last_message_seq) =
        latest_message_cursor(transaction, &principal.room_id).await?;
    let persona_card = resolve_persona_selection(transaction, &draft.persona_card_id).await?;
    let mut session = draft.initial_session(&principal.room_id, AgentRuntimeCustody::Server, now);
    session.public.persona_card = persona_card;
    session
        .public
        .last_seen_event_id
        .clone_from(&last_message_id);
    session.public.last_seen_seq = last_message_seq;
    session.public.last_provider_sync_event_id = last_message_id;
    session.public.last_provider_sync_seq = last_message_seq;
    session.public.bootstrap_cutoff_seq = last_message_seq;
    let public_session = session.public.clone();
    if let Some(operation_id) = start_operation_id {
        prepare_start(&mut session, operation_id);
    }
    insert_agent_authority(transaction, principal, &participant, &session).await?;
    let event =
        append_creation_event(transaction, principal, &participant, &session.public, now).await?;
    let committed_events = vec![event];
    let result = base_result(&public_session, &participant, &committed_events);
    Ok(AgentCreationRecords {
        session,
        result,
        committed_events,
    })
}

async fn insert_agent_authority(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind(&principal.room_id)
    .bind(&participant.participant_id)
    .bind(serde_json::to_string(participant)?)
    .execute(&mut **transaction)
    .await?;
    sqlx::query("INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES (?, ?, ?)")
        .bind(&principal.room_id)
        .bind(&session.public.session_id)
        .bind(serde_json::to_string(session)?)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

async fn append_creation_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
    session: &AgentSession,
    now: chrono::DateTime<Utc>,
) -> Result<RoomEvent, PersistenceError> {
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: now,
        room_id: principal.room_id.clone(),
        event_type: "agent_session_created".to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(session.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(session.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([
            ("session_id".to_owned(), json!(session.session_id)),
            ("provider_kind".to_owned(), json!(session.provider_kind)),
            ("participant".to_owned(), json!(participant)),
            ("agent_session".to_owned(), json!(session)),
        ]),
    };
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
        .bind(&principal.room_id)
        .bind(event.seq)
        .bind(serde_json::to_string(&event)?)
        .execute(&mut **transaction)
        .await?;
    Ok(event)
}

fn base_result(session: &AgentSession, participant: &Participant, events: &[RoomEvent]) -> Value {
    let mut result = json!({
        "status": "created",
        "agent_session": session,
        "participant": participant,
    });
    if let Some(event) = events.last() {
        result["event_seq"] = json!(event.seq);
        result["event"] = json!(event);
        result["events"] = json!(events);
    }
    result
}

fn prepare_start(session: &mut DurableAgentSession, operation_id: &str) {
    session.public.status = AgentSessionStatus::Available;
    session.public.runtime_status = AgentRuntimeStatus::Starting;
    session.public.enabled = true;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    session.lifecycle_intent_action = AgentLifecycleAction::Start;
    operation_id.clone_into(&mut session.lifecycle_intent_id);
    session.lifecycle_intent_status = AgentLifecycleIntentStatus::Prepared;
    session.public.updated_at = Utc::now();
}

async fn save_session(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let changed = update_agent_session_row(transaction, session).await?;
    if changed != 1 {
        return Err(rejected("not_found", "Agent Session was not found."));
    }
    Ok(())
}

pub(crate) async fn latest_message_cursor(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<(String, i64), PersistenceError> {
    let event_json = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.type') = 'message_final' ORDER BY seq DESC LIMIT 1",
    )
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?;
    event_json.map_or_else(
        || Ok((String::new(), 0)),
        |event_json| {
            let event: RoomEvent = serde_json::from_str(&event_json)?;
            Ok((event.id, event.seq))
        },
    )
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}
