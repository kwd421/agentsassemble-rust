//! Terminal request transitions. A response claim is not proof of provider delivery.
use std::collections::BTreeMap;

use agentsassemble_domain::RoomEvent;
use chrono::{DateTime, Utc};
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};
use uuid::Uuid;

use crate::{
    PersistenceError, ProviderRequestDelivery, SqliteStore,
    provider_requests::rejected,
    room_turns::support::{internal_event, load_event},
};

#[derive(Clone, Copy)]
pub enum ProviderRequestDeliveryOutcome {
    Delivered,
    Failed,
}

impl SqliteStore {
    /// Records the live delivery result under its persistence-issued claim.
    ///
    /// # Errors
    /// Rejects changed claims, stale successful delivery and conflicting terminal outcomes.
    pub async fn complete_provider_request_delivery(
        &self,
        delivery: &ProviderRequestDelivery,
        outcome: ProviderRequestDeliveryOutcome,
        now: DateTime<Utc>,
    ) -> Result<RoomEvent, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_request_in(&mut tx, &delivery.room_id, delivery.request_id).await?;
        if row.get::<&str, _>("session_id") != delivery.session_id
            || row.get::<&str, _>("execution_id") != delivery.execution_id
            || u64::try_from(row.get::<i64, _>("turn_generation")).ok()
                != Some(delivery.turn_generation)
            || row
                .get::<Option<Vec<u8>>, _>("resolution_fingerprint")
                .as_deref()
                != Some(delivery.fingerprint.as_slice())
        {
            return Err(PersistenceError::CommandConflict);
        }
        let state = match outcome {
            ProviderRequestDeliveryOutcome::Delivered => "resolved",
            ProviderRequestDeliveryOutcome::Failed => "failed",
        };
        if row.get::<&str, _>("state") != state && row.get::<&str, _>("state") != "resolving" {
            return Err(rejected(
                "provider_request_closed",
                "Provider request is no longer resolving.",
            ));
        }
        if state == "resolved" && row.get::<&str, _>("state") == "resolving" {
            if row.get::<i64, _>("expires_at") <= now.timestamp_millis() {
                return Err(rejected(
                    "provider_request_closed",
                    "Provider request has expired.",
                ));
            }
            let session = crate::agent_lifecycle::load_session(
                &mut tx,
                &delivery.room_id,
                &delivery.session_id,
            )
            .await?;
            crate::provider_request_authority::require_execution_in(&mut tx, &session, &row, now)
                .await?;
        }
        let event = close_in(&mut tx, &row, state).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// Ends a pending request at its stored deadline; early calls have no effect.
    ///
    /// # Errors
    /// Propagates missing requests and persistence failures.
    pub async fn expire_provider_request(
        &self,
        room_id: &str,
        request_id: Uuid,
        now: DateTime<Utc>,
    ) -> Result<Option<RoomEvent>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = load_request_in(&mut tx, room_id, request_id).await?;
        let event = if pending(&row) && row.get::<i64, _>("expires_at") <= now.timestamp_millis() {
            Some(close_in(&mut tx, &row, "expired").await?)
        } else {
            None
        };
        tx.commit().await?;
        Ok(event)
    }

    /// Cancels only requests belonging to the exact execution whose live owner ended.
    ///
    /// # Errors
    /// Propagates persistence failures; never cancels a replacement execution's request.
    pub async fn cancel_provider_execution_requests(
        &self,
        room_id: &str,
        session_id: &str,
        turn_generation: u64,
        execution_id: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let rows = sqlx::query("SELECT * FROM provider_requests WHERE room_id=? AND session_id=? AND turn_generation=? AND execution_id=? AND state IN ('open','resolving')")
            .bind(room_id).bind(session_id)
            .bind(i64::try_from(turn_generation).map_err(|_| PersistenceError::CommandConflict)?)
            .bind(execution_id).fetch_all(&mut *tx).await?;
        let mut events = Vec::new();
        for row in rows {
            events.push(close_in(&mut tx, &row, "cancelled").await?);
        }
        tx.commit().await?;
        Ok(events)
    }
}

async fn load_request_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    request_id: Uuid,
) -> Result<SqliteRow, PersistenceError> {
    sqlx::query("SELECT * FROM provider_requests WHERE room_id=? AND request_id=?")
        .bind(room_id)
        .bind(request_id.to_string())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| {
            rejected(
                "provider_request_missing",
                "Provider request is unavailable.",
            )
        })
}

fn pending(row: &SqliteRow) -> bool {
    matches!(row.get::<&str, _>("state"), "open" | "resolving")
}

async fn close_in(
    tx: &mut Transaction<'_, Sqlite>,
    row: &SqliteRow,
    state: &str,
) -> Result<RoomEvent, PersistenceError> {
    let room_id = row.get("room_id");
    if let Some(id) = row.get::<Option<&str>, _>("closed_event_id") {
        if row.get::<&str, _>("state") != state {
            return Err(PersistenceError::CommandConflict);
        }
        return load_event(tx, room_id, id).await?.ok_or_else(|| {
            rejected(
                "invalid_state",
                "Provider request terminal event is missing.",
            )
        });
    }
    let session = crate::agent_lifecycle::load_session(tx, room_id, row.get("session_id")).await?;
    let event = internal_event(
        tx,
        &session,
        "provider_request_closed",
        true,
        None,
        BTreeMap::from([
            ("visibility".to_owned(), json!("owner")),
            ("owner_id".to_owned(), json!(row.get::<&str, _>("owner_id"))),
            (
                "provider_request_id".to_owned(),
                json!(row.get::<&str, _>("request_id")),
            ),
            ("state".to_owned(), json!(state)),
        ]),
    )
    .await?;
    sqlx::query(
        "UPDATE provider_requests SET state=?, closed_event_id=? WHERE room_id=? AND request_id=?",
    )
    .bind(state)
    .bind(&event.id)
    .bind(room_id)
    .bind(row.get::<&str, _>("request_id"))
    .execute(&mut **tx)
    .await?;
    Ok(event)
}
