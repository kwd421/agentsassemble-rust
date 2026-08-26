use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent, canonical_payload_hash};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    agent_lifecycle_reservations::reject_reserved_request_id, authority::active_room_for_principal,
    participant_leave::PARTICIPANT_LEAVE_ACTION, room_write_budget::reserve_room_write_budget,
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

pub(crate) async fn inspect_non_lifecycle_command(
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

pub(crate) async fn admit_non_lifecycle_command(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
    action: &str,
    payload_hash: &str,
    payload_bytes: usize,
) -> Result<Option<CommandOutcome>, PersistenceError> {
    let outcome = inspect_non_lifecycle_command(
        transaction,
        room_id,
        principal_id,
        request_id,
        action,
        payload_hash,
    )
    .await?;
    if outcome.is_none() {
        reserve_room_write_budget(transaction, room_id, payload_bytes).await?;
    }
    Ok(outcome)
}

impl SqliteStore {
    /// Reports whether one exact new authenticated command should consume the
    /// process-wide principal window before slow validation or provider work.
    ///
    /// Committed replays and matching lifecycle retries never consume another
    /// principal slot. Conflicting request reuse fails visibly.
    ///
    /// # Errors
    ///
    /// Returns authorization, request-conflict, stored-state, or database errors.
    pub async fn command_requires_principal_budget(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<bool, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let existing = existing_request_identity(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
        )
        .await?;
        let self_disabling_leave = action == PARTICIPANT_LEAVE_ACTION
            && !principal.is_operator
            && principal.capabilities.participant_leave
            && payload.as_object().is_some_and(serde_json::Map::is_empty);
        let required = !self_disabling_leave
            && matches!(
                existing,
                None | Some(ExistingRequestIdentity::RejectedLifecycle)
            );
        transaction.commit().await?;
        Ok(required)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExistingRequestIdentity {
    CommittedResult,
    PendingLifecycle,
    RejectedLifecycle,
}

pub(crate) async fn existing_request_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
    action: &str,
    payload_hash: &str,
) -> Result<Option<ExistingRequestIdentity>, PersistenceError> {
    let command = sqlx::query(
        "SELECT action, payload_hash FROM command_results WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let reservation = sqlx::query(
        "SELECT action, payload_hash, status FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if command.is_some() && reservation.is_some() {
        return Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            message: "A room request has conflicting durable owners.".to_owned(),
        });
    }
    if let Some(row) = command {
        validate_request_identity(&row, action, payload_hash)?;
        return Ok(Some(ExistingRequestIdentity::CommittedResult));
    }
    let Some(row) = reservation else {
        return Ok(None);
    };
    validate_request_identity(&row, action, payload_hash)?;
    match row.try_get::<String, _>("status")?.as_str() {
        "pending" => Ok(Some(ExistingRequestIdentity::PendingLifecycle)),
        "rejected" => Ok(Some(ExistingRequestIdentity::RejectedLifecycle)),
        _ => Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            message: "A lifecycle request has an invalid durable status.".to_owned(),
        }),
    }
}

fn validate_request_identity(
    row: &sqlx::sqlite::SqliteRow,
    action: &str,
    payload_hash: &str,
) -> Result<(), PersistenceError> {
    if row.try_get::<String, _>("action")? != action
        || row.try_get::<String, _>("payload_hash")? != payload_hash
    {
        return Err(PersistenceError::CommandConflict);
    }
    Ok(())
}
