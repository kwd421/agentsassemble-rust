use agentsassemble_domain::AuthenticatedPrincipal;
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError,
    room_write_budget::{command_size, reserve_room_write_budget},
};

pub(crate) struct LifecycleReservation<'a> {
    pub principal: &'a AuthenticatedPrincipal,
    pub request_id: &'a str,
    pub action: &'a str,
    pub payload_hash: &'a str,
    pub session_id: &'a str,
    pub operation_id: &'a str,
    pub phase: &'a str,
    pub prepared_result_json: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StoredLifecycleReservation {
    pub action: String,
    pub payload_hash: String,
    pub session_id: String,
    pub operation_id: String,
    pub status: String,
    pub phase: String,
    pub prepared_result_json: String,
}

impl<'a> LifecycleReservation<'a> {
    pub(crate) const fn new(
        principal: &'a AuthenticatedPrincipal,
        request_id: &'a str,
        action: &'a str,
        payload_hash: &'a str,
        session_id: &'a str,
        operation_id: &'a str,
    ) -> Self {
        Self {
            principal,
            request_id,
            action,
            payload_hash,
            session_id,
            operation_id,
            phase: "lifecycle_prepared",
            prepared_result_json: "{}",
        }
    }

    pub(crate) const fn creation(
        principal: &'a AuthenticatedPrincipal,
        request_id: &'a str,
        payload_hash: &'a str,
        session_id: &'a str,
        operation_id: &'a str,
        prepared_result_json: &'a str,
    ) -> Self {
        Self {
            principal,
            request_id,
            action: "agent.create",
            payload_hash,
            session_id,
            operation_id,
            phase: "creation_committed",
            prepared_result_json,
        }
    }
}

pub(crate) async fn load_lifecycle_reservation(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
) -> Result<Option<StoredLifecycleReservation>, PersistenceError> {
    let row = sqlx::query(
        "SELECT action, payload_hash, session_id, operation_id, status, phase, prepared_result_json FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    Ok(row.map(|row| StoredLifecycleReservation {
        action: row.get("action"),
        payload_hash: row.get("payload_hash"),
        session_id: row.get("session_id"),
        operation_id: row.get("operation_id"),
        status: row.get("status"),
        phase: row.get("phase"),
        prepared_result_json: row.get("prepared_result_json"),
    }))
}

pub(crate) async fn claim_lifecycle_command(
    transaction: &mut Transaction<'_, Sqlite>,
    reservation: &LifecycleReservation<'_>,
    payload: &Value,
) -> Result<(), PersistenceError> {
    let existing = sqlx::query(
        "SELECT action, payload_hash, session_id, operation_id, status, phase, prepared_result_json FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ?",
    )
    .bind(&reservation.principal.room_id)
    .bind(&reservation.principal.principal_id)
    .bind(reservation.request_id)
    .fetch_optional(&mut **transaction)
    .await?;
    if let Some(row) = existing {
        let matches = row.get::<String, _>("action") == reservation.action
            && row.get::<String, _>("payload_hash") == reservation.payload_hash
            && row.get::<String, _>("session_id") == reservation.session_id
            && row.get::<String, _>("operation_id") == reservation.operation_id
            && row.get::<String, _>("phase") == reservation.phase
            && row.get::<String, _>("prepared_result_json") == reservation.prepared_result_json;
        if !matches {
            return Err(PersistenceError::CommandConflict);
        }
        return match row.get::<String, _>("status").as_str() {
            "pending" => Ok(()),
            "owner_lost" => Err(PersistenceError::CommandRejected {
                code: "runtime_owner_lost",
                message: "The original provider runtime owner was lost during restart. Use a new lifecycle request.".to_owned(),
            }),
            _ => Err(invalid_reservation()),
        };
    }
    if reservation.action != "agent.stop" {
        reserve_room_write_budget(
            transaction,
            &reservation.principal.room_id,
            command_size(reservation.request_id, reservation.action, payload)?,
        )
        .await?;
    }
    sqlx::query(
        "INSERT INTO lifecycle_command_reservations(room_id, principal_id, request_id, action, payload_hash, session_id, operation_id, status, phase, prepared_result_json) VALUES (?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
    )
    .bind(&reservation.principal.room_id)
    .bind(&reservation.principal.principal_id)
    .bind(reservation.request_id)
    .bind(reservation.action)
    .bind(reservation.payload_hash)
    .bind(reservation.session_id)
    .bind(reservation.operation_id)
    .bind(reservation.phase)
    .bind(reservation.prepared_result_json)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

pub(crate) async fn mark_lifecycle_owner_lost(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    session_id: &str,
    action: &str,
    operation_id: &str,
) -> Result<(), PersistenceError> {
    let updated = sqlx::query(
        "UPDATE lifecycle_command_reservations SET status = 'owner_lost' WHERE room_id = ? AND session_id = ? AND operation_id = ? AND action = ? AND status = 'pending'",
    )
    .bind(room_id)
    .bind(session_id)
    .bind(operation_id)
    .bind(action)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if updated != 1 {
        return Err(invalid_reservation());
    }
    Ok(())
}

pub(crate) async fn reject_reserved_request_id(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
) -> Result<(), PersistenceError> {
    let reserved = sqlx::query_scalar::<_, i64>(
        "SELECT EXISTS(SELECT 1 FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ?)",
    )
    .bind(room_id)
    .bind(principal_id)
    .bind(request_id)
    .fetch_one(&mut **transaction)
    .await?;
    if reserved != 0 {
        return Err(PersistenceError::CommandConflict);
    }
    Ok(())
}

pub(crate) async fn finish_lifecycle_command(
    transaction: &mut Transaction<'_, Sqlite>,
    reservation: &LifecycleReservation<'_>,
) -> Result<(), PersistenceError> {
    let removed = sqlx::query(
        "DELETE FROM lifecycle_command_reservations WHERE room_id = ? AND principal_id = ? AND request_id = ? AND action = ? AND payload_hash = ? AND session_id = ? AND operation_id = ? AND status = 'pending' AND phase = ? AND prepared_result_json = ?",
    )
    .bind(&reservation.principal.room_id)
    .bind(&reservation.principal.principal_id)
    .bind(reservation.request_id)
    .bind(reservation.action)
    .bind(reservation.payload_hash)
    .bind(reservation.session_id)
    .bind(reservation.operation_id)
    .bind(reservation.phase)
    .bind(reservation.prepared_result_json)
    .execute(&mut **transaction)
    .await?
    .rows_affected();
    if removed != 1 {
        return Err(invalid_reservation());
    }
    Ok(())
}

fn invalid_reservation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "stale_lifecycle_reservation",
        message: "Provider lifecycle command reservation is missing or inconsistent.".to_owned(),
    }
}
