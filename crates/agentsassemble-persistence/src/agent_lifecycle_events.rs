use std::collections::BTreeMap;

use agentsassemble_domain::{Actor, AgentSession, AuthenticatedPrincipal, RoomEvent};
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{CommandOutcome, PersistenceError, command_admission::store_command_result};

pub(crate) async fn append_session_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    session: &AgentSession,
    event_type: &str,
    extra: BTreeMap<String, Value>,
) -> Result<RoomEvent, PersistenceError> {
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: Utc::now(),
        room_id: principal.room_id.clone(),
        event_type: event_type.to_owned(),
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
        extra,
    };
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
        .bind(&principal.room_id)
        .bind(event.seq)
        .bind(serde_json::to_string(&event)?)
        .execute(&mut **transaction)
        .await?;
    Ok(event)
}

pub(crate) async fn append_state_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    session: &AgentSession,
) -> Result<RoomEvent, PersistenceError> {
    append_session_event(
        transaction,
        principal,
        session,
        "agent_session_state",
        BTreeMap::from([
            ("session_id".to_owned(), json!(session.session_id)),
            ("runtime_status".to_owned(), json!(session.runtime_status)),
            ("agent_session".to_owned(), json!(session)),
        ]),
    )
    .await
}

pub(crate) async fn append_error_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    session: &AgentSession,
    error_code: &str,
    message: &str,
) -> Result<RoomEvent, PersistenceError> {
    let mut event = append_session_event(
        transaction,
        principal,
        session,
        "error",
        BTreeMap::from([("error_code".to_owned(), json!(error_code))]),
    )
    .await?;
    event.content = Some(message.to_owned());
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = ? AND seq = ?")
        .bind(serde_json::to_string(&event)?)
        .bind(&principal.room_id)
        .bind(event.seq)
        .execute(&mut **transaction)
        .await?;
    Ok(event)
}

pub(crate) async fn commit_already_stopped(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: String,
    session: &AgentSession,
) -> Result<CommandOutcome, PersistenceError> {
    let event = append_state_event(transaction, principal, session).await?;
    let events = vec![event];
    let result = json!({
        "agent_session": session,
        "process": {
            "stopped": true,
            "alive": false,
            "ownership": "server",
            "already_stopped": true,
        },
        "revoked_sessions": 0,
        "events": events,
        "event": events.last(),
    });
    store_result(
        transaction,
        principal,
        request_id,
        "agent.stop",
        payload_hash,
        result,
        events,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn store_result(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    action: &str,
    payload_hash: String,
    result: Value,
    events: Vec<RoomEvent>,
) -> Result<CommandOutcome, PersistenceError> {
    let event = events
        .last()
        .cloned()
        .ok_or_else(|| PersistenceError::CommandRejected {
            code: "invalid_state",
            message: "Command outcome has no event.".to_owned(),
        })?;
    store_command_result(
        transaction,
        principal,
        request_id,
        action,
        &payload_hash,
        &result,
    )
    .await?;
    Ok(CommandOutcome {
        result,
        event,
        events,
        deduplicated: false,
    })
}

async fn next_sequence(
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
