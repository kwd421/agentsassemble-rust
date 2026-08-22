use agentsassemble_domain::RoomEvent;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, agent_lifecycle_reservations::reject_reserved_request_id,
};

pub(crate) async fn existing_command(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
    action: &str,
    payload_hash: &str,
) -> Result<Option<CommandOutcome>, PersistenceError> {
    let row = sqlx::query(
        "SELECT action, payload_hash, result_json FROM command_results WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let stored_action: String = row.try_get("action")?;
    let stored_hash: String = row.try_get("payload_hash")?;
    if stored_action != action || stored_hash != payload_hash {
        return Err(PersistenceError::CommandConflict);
    }
    let result: Value = serde_json::from_str(row.try_get::<String, _>("result_json")?.as_str())?;
    let event: RoomEvent =
        serde_json::from_value(result.get("event").cloned().ok_or_else(|| {
            serde_json::Error::io(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stored command result has no event",
            ))
        })?)?;
    let events = result.get("events").map_or_else(
        || Ok(vec![event.clone()]),
        |events| serde_json::from_value(events.clone()),
    )?;
    Ok(Some(CommandOutcome {
        result,
        event,
        events,
        deduplicated: true,
    }))
}

pub(crate) async fn admit_non_lifecycle_command(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
    action: &str,
    payload_hash: &str,
) -> Result<Option<CommandOutcome>, PersistenceError> {
    let outcome = existing_command(
        transaction,
        room_id,
        principal_id,
        request_id,
        action,
        payload_hash,
    )
    .await?;
    reject_reserved_request_id(transaction, room_id, principal_id, request_id).await?;
    Ok(outcome)
}
