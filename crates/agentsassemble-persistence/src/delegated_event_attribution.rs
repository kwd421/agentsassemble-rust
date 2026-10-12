//! Executor/authorizer attribution for the events created by one delegated transaction.
use crate::PersistenceError;
use agentsassemble_domain::{Actor, AuthenticatedPrincipal, RoomEvent};
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

fn attribute(event: &mut RoomEvent, principal: &AuthenticatedPrincipal, owner_id: &str) {
    event.actor = Actor {
        participant_id: principal.participant_id.clone(),
        participant_type: "agent".into(),
    };
    event.actor_id = Some(principal.participant_id.clone());
    event.actor_type = Some("agent".into());
    event.extra.insert(
        "actor_participant_id".into(),
        json!(principal.participant_id),
    );
    event
        .extra
        .insert("on_behalf_of_participant_id".into(), json!(owner_id));
    // Settings events name the executor in participant fields; other events name the target.
    if event.event_type == "room_settings_updated" {
        event.participant_id = Some(principal.participant_id.clone());
        event.participant_type = Some("agent".into());
        event.display_name = Some(principal.display_name.clone());
    }
}

pub(crate) async fn attribute_since(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    owner_id: &str,
    first_seq: i64,
    returned: &mut [RoomEvent],
) -> Result<(), PersistenceError> {
    // The mutation transaction owns this sequence range, including events not in its ACK
    // (private request cancellation is published by durable catch-up). No other writer enters it.
    let rows: Vec<String> = sqlx::query_scalar(
        "SELECT event_json FROM room_events WHERE room_id=? AND seq>=? ORDER BY seq",
    )
    .bind(&principal.room_id)
    .bind(first_seq)
    .fetch_all(&mut **tx)
    .await?;
    for encoded in rows {
        let mut event: RoomEvent = serde_json::from_str(&encoded)?;
        attribute(&mut event, principal, owner_id);
        sqlx::query("UPDATE room_events SET event_json=? WHERE room_id=? AND seq=?")
            .bind(serde_json::to_string(&event)?)
            .bind(&principal.room_id)
            .bind(event.seq)
            .execute(&mut **tx)
            .await?;
    }
    for event in returned {
        attribute(event, principal, owner_id);
    }
    Ok(())
}

pub(crate) async fn control_events(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    payload: &Value,
    first_seq: i64,
    returned: &mut [RoomEvent],
) -> Result<(), PersistenceError> {
    if let Some(metadata) = payload.get("_delegation") {
        let attribution: crate::delegated_agent_control::Attribution =
            serde_json::from_value(metadata.clone())?;
        attribute_since(
            tx,
            principal,
            &attribution.on_behalf_of_participant_id,
            first_seq,
            returned,
        )
        .await?;
    }
    Ok(())
}

// Unconfirmed effects carry only the exact operation ID; the durable reservation owns
// its original executor/authorizer. Read that payload, never a current replacement binding.
pub(crate) async fn recorded_control(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    operation_id: &str,
) -> Result<(AuthenticatedPrincipal, Value), PersistenceError> {
    let (principal, payload): (String, String) = sqlx::query_as(
        "SELECT principal_json,payload_json FROM lifecycle_command_reservations WHERE room_id=? AND session_id=? AND operation_id=? AND status='pending'")
        .bind(room_id).bind(session_id).bind(operation_id).fetch_one(&mut **tx).await?;
    Ok((
        serde_json::from_str(&principal)?,
        serde_json::from_str(&payload)?,
    ))
}
