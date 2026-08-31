use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AgentSession, AgentSessionDraft, AuthenticatedPrincipal,
    CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession, Participant, ParticipantRole,
    ParticipantStatus, RoomEvent,
};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, persona_library::resolve_persona_selection,
    room_event_sequence::next_sequence, sqlite::MAX_AGENT_SESSIONS_PER_ROOM,
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
        load_optional_session(transaction, &principal.room_id, &draft.agent_id).await?;
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
            if !session.lifecycle_intent_action.is_empty()
                || !session.lifecycle_intent_id.is_empty()
                || !session.lifecycle_intent_status.is_empty()
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
            "An Agent Session with this identity already exists; re-add or configure the existing session instead.",
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
    let public_session = AgentSession {
        room_id: principal.room_id.clone(),
        session_id: draft.agent_id.clone(),
        participant_id: draft.agent_id.clone(),
        display_name: draft.display_name.clone(),
        status: "available".to_owned(),
        runtime_status: "stopped".to_owned(),
        enabled: false,
        provider_kind: draft.provider_kind.clone(),
        runtime_kind: draft.runtime_kind.clone(),
        connection_kind: draft.connection_kind.clone(),
        external_owned: false,
        process_ownership: "server".to_owned(),
        model: draft.model.clone(),
        reasoning_effort: draft.reasoning_effort.clone(),
        service_tier: draft.service_tier.clone(),
        variant: draft.variant.clone(),
        execution_harness: draft.execution_harness.clone(),
        permission_mode: draft.permission_mode.clone(),
        max_output_tokens: draft.max_output_tokens,
        catalog_revision: draft.catalog_revision.clone(),
        persona_card_id: draft.persona_card_id.clone().into_boxed_str(),
        persona_card,
        transport: draft.transport.clone(),
        last_seen_event_id: last_message_id.clone(),
        last_seen_seq: last_message_seq,
        last_provider_sync_event_id: last_message_id,
        last_provider_sync_seq: last_message_seq,
        bootstrap_cutoff_seq: last_message_seq,
        turn_count: 0,
        active_turn_id: String::new(),
        turn_phase: String::new(),
        last_error: String::new(),
        last_error_code: String::new(),
        recovery_required: false,
        provider_session_active: false,
        provider_session_reused: false,
        created_at: now,
        updated_at: now,
    };
    let mut session = new_durable_session(public_session.clone(), draft);
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

fn new_durable_session(public: AgentSession, draft: &AgentSessionDraft) -> DurableAgentSession {
    DurableAgentSession {
        public,
        executable: draft.executable.clone(),
        executable_identity: draft.executable_identity.clone(),
        workspace: draft.workspace.clone(),
        workspace_identity: draft.workspace_identity.clone(),
        runtime_profile_key: draft.runtime_profile_key.clone(),
        runtime_profile_version: CURRENT_RUNTIME_PROFILE_VERSION,
        provider_session_id: String::new(),
        runtime_handle_id: String::new(),
        runtime_owner_id: String::new(),
        runtime_lease_token: String::new(),
        turn_generation: 0,
        schedule_requested: false,
        pending_inputs: Vec::new(),
        inflight_inputs: Vec::new(),
        active_source_event_id: String::new(),
        input_up_to_event_id: String::new(),
        input_up_to_seq: 0,
        lifecycle_intent_action: String::new(),
        lifecycle_intent_id: String::new(),
        lifecycle_intent_status: String::new(),
    }
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
    "available".clone_into(&mut session.public.status);
    "starting".clone_into(&mut session.public.runtime_status);
    session.public.enabled = true;
    session.public.last_error.clear();
    session.public.last_error_code.clear();
    session.public.recovery_required = false;
    "start".clone_into(&mut session.lifecycle_intent_action);
    operation_id.clone_into(&mut session.lifecycle_intent_id);
    "prepared".clone_into(&mut session.lifecycle_intent_status);
    session.public.updated_at = Utc::now();
}

async fn load_optional_session(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
) -> Result<Option<DurableAgentSession>, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT session_json FROM agent_sessions WHERE room_id = ? AND session_id = ?",
    )
    .bind(room_id)
    .bind(session_id)
    .fetch_optional(&mut **transaction)
    .await?;
    encoded
        .map(|encoded| serde_json::from_str(&encoded))
        .transpose()
        .map_err(Into::into)
}

async fn load_optional_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Option<Participant>, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?;
    encoded
        .map(|encoded| serde_json::from_str(&encoded))
        .transpose()
        .map_err(Into::into)
}

async fn save_session(
    transaction: &mut Transaction<'_, Sqlite>,
    session: &DurableAgentSession,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE agent_sessions SET session_json = ? WHERE room_id = ? AND session_id = ?",
    )
    .bind(serde_json::to_string(session)?)
    .bind(&session.public.room_id)
    .bind(&session.public.session_id)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if changed != 1 {
        return Err(rejected("not_found", "Agent Session was not found."));
    }
    Ok(())
}

async fn latest_message_cursor(
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
        code,
        message: message.into(),
    }
}
