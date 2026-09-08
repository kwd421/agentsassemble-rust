use crate::{PersistenceError, attendee_invites::rejected};
use agentsassemble_domain::{
    Actor, AgentRuntimeStatus, AgentSession, AgentSessionStatus, AgentTurnPhase, Participant,
    ParticipantRole, ParticipantStatus, RoomEvent,
};
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(crate) async fn insert_membership(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    owner_id: &str,
    provider_kind: &str,
    display_name: &str,
    now: DateTime<Utc>,
) -> Result<RoomEvent, PersistenceError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agent_sessions WHERE room_id=?")
        .bind(room_id)
        .fetch_one(&mut **tx)
        .await?;
    if count >= crate::sqlite::MAX_AGENT_SESSIONS_PER_ROOM {
        return Err(rejected(
            "agent_session_capacity",
            "This room has reached its Agent Session capacity.",
        ));
    }
    let participant = Participant {
        room_id: room_id.to_owned(),
        participant_id: format!("attendee-{}", Uuid::new_v4().simple()),
        display_name: display_name.to_owned(),
        avatar_image_url: String::new(),
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Agent,
        owner_id: owner_id.to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    let session = initial_session(tx, &participant, provider_kind).await?;
    sqlx::query("INSERT INTO participants(room_id,participant_id,participant_json) VALUES(?,?,?)")
        .bind(room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut **tx)
        .await?;
    sqlx::query("INSERT INTO agent_sessions(room_id,session_id,session_json) VALUES(?,?,?)")
        .bind(room_id)
        .bind(&session.public.session_id)
        .bind(serde_json::to_string(&session)?)
        .execute(&mut **tx)
        .await?;
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(tx, room_id).await?,
        created_at: now,
        room_id: room_id.to_owned(),
        event_type: "agent_session_created".to_owned(),
        actor: Actor {
            participant_id: participant.participant_id.clone(),
            participant_type: "agent".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(participant.participant_id.clone()),
        actor_type: Some("agent".to_owned()),
        display_name: Some(display_name.to_owned()),
        content: None,
        message_kind: None,
        extra: std::collections::BTreeMap::from([
            ("session_id".to_owned(), json!(session.public.session_id)),
            ("provider_kind".to_owned(), json!(provider_kind)),
            ("participant".to_owned(), json!(participant)),
            ("agent_session".to_owned(), json!(session.public)),
        ]),
    };
    crate::room_turns::support::insert_event(tx, &event).await?;
    Ok(event)
}

async fn initial_session(
    tx: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
    provider_kind: &str,
) -> Result<agentsassemble_domain::DurableAgentSession, PersistenceError> {
    let (last_id, last_seq) =
        crate::agent_creation_records::latest_message_cursor(tx, &participant.room_id).await?;
    let public = AgentSession {
        avatar_image_url: String::new(),
        room_id: participant.room_id.clone(),
        session_id: participant.participant_id.clone(),
        participant_id: participant.participant_id.clone(),
        display_name: participant.display_name.clone(),
        status: AgentSessionStatus::Attached,
        runtime_status: AgentRuntimeStatus::Disconnected,
        enabled: false,
        provider_kind: provider_kind.to_owned(),
        runtime_kind: "external_attendee".to_owned(),
        connection_kind: "canonical_room_websocket".to_owned(),
        external_owned: true,
        process_ownership: "external".to_owned(),
        model: String::new(),
        reasoning_effort: String::new(),
        service_tier: String::new(),
        variant: String::new(),
        execution_harness: String::new(),
        permission_mode: "participant".to_owned(),
        max_output_tokens: 0,
        catalog_revision: String::new(),
        persona_card_id: String::new().into_boxed_str(),
        persona_card: None,
        transport: "websocket".to_owned(),
        last_seen_event_id: last_id.clone(),
        last_seen_seq: last_seq,
        last_provider_sync_event_id: last_id,
        last_provider_sync_seq: last_seq,
        bootstrap_cutoff_seq: last_seq,
        turn_count: 0,
        active_turn_id: String::new(),
        turn_phase: AgentTurnPhase::None,
        last_error: String::new(),
        last_error_code: String::new(),
        recovery_required: false,
        provider_session_active: false,
        provider_session_reused: false,
        created_at: participant.created_at,
        updated_at: participant.updated_at,
    };
    Ok(crate::agent_session_rows::without_runtime(public))
}
