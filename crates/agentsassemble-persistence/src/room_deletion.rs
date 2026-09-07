use agentsassemble_domain::{AuthenticatedPrincipal, RoomStatus, canonical_payload_hash};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    command_admission::admit_non_lifecycle_command,
    room_lifecycle::{
        lifecycle_event, request_room_cleanup, require_manager_membership, resolve_local_owner,
        revoke_room_access, room_cleanup_pending,
    },
    room_turns::support::{insert_event, load_room_with_settings},
    room_write_budget::command_size,
};

/// Pending deletion has committed revocation, but never reports completed deletion.
#[derive(Debug)]
pub struct RoomDeletionMutation {
    pub outcome: CommandOutcome,
    pub complete: bool,
    pub revoked_session_fingerprints: Vec<[u8; 32]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeletePayload {
    room_uid: Uuid,
    confirmation_name: String,
}

impl SqliteStore {
    /// Authenticates current local ownership and exact retained deletion identity.
    ///
    /// # Errors
    /// Rejects foreign authority, conflicting replay or absent fresh membership.
    pub async fn resolve_room_delete_principal(
        &self,
        credential: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        parse_payload(payload)?;
        let (principal, _) = resolve_delete_request(
            &mut transaction,
            credential,
            request_id,
            &canonical_payload_hash(payload),
        )
        .await?;
        transaction.commit().await?;
        Ok(principal)
    }

    /// Records a closed room and exact deletion intent before any destructive effect.
    ///
    /// # Errors
    /// Requires current-name confirmation, local ownership and an exact incarnation.
    pub async fn execute_room_delete(
        &self,
        credential: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<RoomDeletionMutation, PersistenceError> {
        let parsed = parse_payload(payload)?;
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        let (principal, replay) =
            resolve_delete_request(&mut transaction, credential, request_id, &payload_hash).await?;
        if let Some(mutation) = replay {
            transaction.commit().await?;
            return Ok(mutation);
        }
        let (mut room, _) = load_room_with_settings(&mut transaction, &principal.room_id).await?;
        if room.room_uid != parsed.room_uid {
            return Err(rejected(
                "room_incarnation_changed",
                "The exact room no longer exists.",
            ));
        }
        if room.label != parsed.confirmation_name {
            return Err(rejected(
                "confirmation_mismatch",
                "The confirmation name must match the current room name.",
            ));
        }
        if sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM room_delete_results WHERE room_uid = ?)",
        )
        .bind(room.room_uid.to_string())
        .fetch_one(&mut *transaction)
        .await?
        {
            return Err(rejected(
                "room_deletion_pending",
                "This room already has an exact deletion request.",
            ));
        }
        // A deletion result lives outside the cascade; ordinary request IDs still
        // conflict with all currently retained room command owners before admission.
        if admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            "room.delete",
            &payload_hash,
            command_size(request_id, "room.delete", payload)?,
        )
        .await?
        .is_some()
        {
            return Err(PersistenceError::CommandConflict);
        }
        request_room_cleanup(&mut transaction, &room.room_id).await?;
        let revoked_session_fingerprints =
            revoke_room_access(&mut transaction, &room.room_id).await?;
        room.status = RoomStatus::Closed;
        room.updated_at = Utc::now();
        sqlx::query("UPDATE rooms SET room_json = ? WHERE room_id = ?")
            .bind(serde_json::to_string(&room)?)
            .bind(&room.room_id)
            .execute(&mut *transaction)
            .await?;
        let event = lifecycle_event(&mut transaction, &principal, &room).await?;
        insert_event(&mut transaction, &event).await?;
        let result = json!({"room": room, "deleted": true, "event": event, "event_seq": event.seq, "events": [event]});
        sqlx::query("INSERT INTO room_delete_results(room_id, room_uid, principal_id, request_id, payload_hash, result_json, state) VALUES (?, ?, ?, ?, ?, ?, 'pending')")
            .bind(&room.room_id).bind(room.room_uid.to_string()).bind(&principal.principal_id)
            .bind(request_id).bind(&payload_hash).bind(serde_json::to_string(&result)?)
            .execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(RoomDeletionMutation {
            outcome: CommandOutcome {
                result,
                event: event.clone(),
                events: vec![event],
                deduplicated: false,
            },
            complete: false,
            revoked_session_fingerprints,
        })
    }

    /// Returns a bounded keyset page for the existing recovery watcher.
    ///
    /// # Errors
    /// Storage failure is not an empty page.
    pub async fn pending_room_deletions(
        &self,
        after: Option<&str>,
    ) -> Result<Vec<String>, PersistenceError> {
        Ok(sqlx::query_scalar("SELECT room_id FROM room_delete_results WHERE state = 'pending' AND (? IS NULL OR room_id > ?) ORDER BY room_id LIMIT 64")
            .bind(after).bind(after).fetch_all(&self.pool).await?)
    }

    /// Deletes only after exact runtime custody is cleared and closure was published.
    /// Room-owned assets cascade in this transaction; the immutable result survives.
    ///
    /// # Errors
    /// Fails on changed incarnation/state or database errors; pending work is retained.
    pub async fn finish_room_deletion(&self, room_id: &str) -> Result<bool, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query("SELECT room_uid, result_json FROM room_delete_results WHERE room_id = ? AND state = 'pending'")
            .bind(room_id).fetch_optional(&mut *transaction).await?;
        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(false);
        };
        let (room, _) = load_room_with_settings(&mut transaction, room_id).await?;
        if room.room_uid.to_string() != row.try_get::<String, _>("room_uid")?
            || room.status != RoomStatus::Closed
        {
            return Err(rejected(
                "invalid_state",
                "Pending deletion no longer matches its closed room.",
            ));
        }
        let result: Value = serde_json::from_str(row.try_get("result_json")?)?;
        let seq = result["event_seq"].as_i64().ok_or_else(|| {
            rejected("invalid_state", "Deletion publication identity is invalid.")
        })?;
        let published: Option<i64> = sqlx::query_scalar(
            "SELECT published_seq FROM room_event_publication_cursors WHERE room_id = ?",
        )
        .bind(room_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if room_cleanup_pending(&mut transaction, room_id).await?
            || published.is_none_or(|cursor| cursor < seq)
        {
            transaction.commit().await?;
            return Ok(false);
        }
        sqlx::query("DELETE FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE room_delete_results SET state = 'complete' WHERE room_id = ? AND state = 'pending'")
            .bind(room_id).execute(&mut *transaction).await?;
        transaction.commit().await?;
        Ok(true)
    }
}

pub(crate) async fn resolve_delete_request(
    transaction: &mut Transaction<'_, Sqlite>,
    credential: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: &str,
) -> Result<(AuthenticatedPrincipal, Option<RoomDeletionMutation>), PersistenceError> {
    let principal = resolve_local_owner(transaction, credential).await?;
    let replay = deletion_replay(transaction, &principal, request_id, payload_hash).await?;
    if replay.is_some() {
        Ok((principal, replay))
    } else {
        Ok((
            require_manager_membership(transaction, principal).await?,
            None,
        ))
    }
}

pub(crate) async fn deletion_replay(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload_hash: &str,
) -> Result<Option<RoomDeletionMutation>, PersistenceError> {
    let row = sqlx::query("SELECT payload_hash, result_json, state FROM room_delete_results WHERE room_id = ? AND principal_id = ? AND request_id = ?")
        .bind(&principal.room_id).bind(&principal.principal_id).bind(request_id)
        .fetch_optional(&mut **transaction).await?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<String, _>("payload_hash")? != payload_hash {
        return Err(PersistenceError::CommandConflict);
    }
    let result: Value = serde_json::from_str(row.try_get("result_json")?)?;
    let event = serde_json::from_value(result["event"].clone())?;
    let events = serde_json::from_value(result["events"].clone())?;
    Ok(Some(RoomDeletionMutation {
        outcome: CommandOutcome {
            result,
            event,
            events,
            deduplicated: true,
        },
        complete: row.try_get::<String, _>("state")? == "complete",
        revoked_session_fingerprints: Vec::new(),
    }))
}

fn parse_payload(payload: &Value) -> Result<DeletePayload, PersistenceError> {
    serde_json::from_value(payload.clone()).map_err(|_| {
        rejected(
            "bad_request",
            "An exact room_uid and confirmation_name are required.",
        )
    })
}

pub(crate) async fn reject_deleted_request_reuse(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    principal_id: &str,
    request_id: &str,
) -> Result<(), PersistenceError> {
    if sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM room_delete_results WHERE room_id = ? AND principal_id = ? AND request_id = ?)")
        .bind(room_id).bind(principal_id).bind(request_id).fetch_one(&mut **transaction).await? {
        return Err(PersistenceError::CommandConflict);
    }
    Ok(())
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
