//! Durable action binding and request admission; all public details belong to the human owner.
use crate::{
    DelegateRoomAuthority, PersistenceError, SqliteStore,
    delegated_pin::DelegatedRoomActionAuthority,
};
use agentsassemble_domain::{
    ConfirmationState, ConfirmedRoomAction, DelegatedActionRequest, DeleteTextChannel,
    RoomSettings, public_settings,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS delegated_action_requests (room_id TEXT NOT NULL, confirmation_id TEXT NOT NULL, owner_id TEXT NOT NULL, delegate_id TEXT NOT NULL, request_id TEXT NOT NULL, created_at INTEGER NOT NULL, expires_at INTEGER NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending','declined','expired','cancelled','executing','succeeded','failed')), binding_json TEXT NOT NULL, request_json TEXT NOT NULL, decision_json TEXT, receipt_json TEXT, PRIMARY KEY(room_id,confirmation_id), UNIQUE(room_id,delegate_id,request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE) STRICT";

#[derive(Serialize, Deserialize)]
pub(crate) struct Binding {
    pub(crate) room_uid: Uuid,
    pub(crate) authority: DelegateRoomAuthority,
    pub(crate) owner_reference: Value,
}

pub(crate) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}

pub(crate) async fn settings_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<RoomSettings, PersistenceError> {
    let encoded: String = sqlx::query_scalar("SELECT settings_json FROM rooms WHERE room_id=?")
        .bind(room_id)
        .fetch_one(&mut **tx)
        .await?;
    Ok(serde_json::from_str(&encoded)?)
}

impl SqliteStore {
    /// Creates only an immutable confirmation request. Exact retries never debit again.
    /// # Errors
    /// Rejects stale custody, invalid targets, conflicting retries and exhausted budgets.
    pub async fn request_delegated_channel_delete<A>(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DeleteTextChannel,
        admit: impl FnOnce(&str, &str, usize) -> Result<A, PersistenceError>,
    ) -> Result<(Value, Option<A>), PersistenceError> {
        if !Uuid::parse_str(&request.request_id).is_ok_and(|id| !id.is_nil()) {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let action = ConfirmedRoomAction::DeleteTextChannel(request.clone());
        if let Some(result) = crate::delegated_confirmation_lifecycle::retry_in(
            &mut tx,
            authority,
            &request.request_id,
            &action,
        )
        .await?
        {
            tx.commit().await?;
            return Ok((result, None));
        }
        let proof =
            DelegatedRoomActionAuthority::direct(&mut tx, authority, "request_delete_text_channel")
                .await?;
        crate::command_admission::existing_request_identity(
            &mut tx,
            authority.room_id(),
            &proof.delegate.principal_id,
            &request.request_id,
            "request_delete_text_channel",
            &agentsassemble_domain::canonical_payload_hash(&serde_json::to_value(request)?),
        )
        .await
        .map_err(|error| match error {
            PersistenceError::CommandConflict => rejected(
                "request_conflict",
                "This request ID already identifies another action.",
            ),
            other => other,
        })?;
        let current = settings_in(&mut tx, authority.room_id()).await?;
        if public_settings(&current)?.settings_revision != request.expected_settings_revision {
            return Err(rejected("target_changed", "Room settings have changed."));
        }
        crate::room_channels::require_text_channel(
            &mut tx,
            authority.room_id(),
            &request.channel_id,
        )
        .await?;
        let target = current
            .channels
            .iter()
            .find(|c| c.id == request.channel_id)
            .ok_or_else(|| rejected("channel_not_found", "Text channel is unavailable."))?;
        let now = Utc::now();
        let room = crate::authority::load_active_room(&mut tx, authority.room_id()).await?;
        let (owner_reference, owner_expiry) = crate::delegation_owner::confirmation_provenance(
            &mut tx,
            authority.room_id(),
            authority.delegate_id(),
        )
        .await?;
        let mut expiry = now + chrono::Duration::minutes(5);
        for bound in [
            owner_expiry,
            match authority {
                DelegateRoomAuthority::Provider { .. } => None,
                DelegateRoomAuthority::Attendee { session, .. } => Some(session.expires_at()),
                DelegateRoomAuthority::Connector(session) => Some(session.expires_at()),
            },
        ]
        .into_iter()
        .flatten()
        {
            expiry = expiry.min(bound);
        }
        let pending: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delegated_action_requests WHERE room_id=? AND owner_id=? AND state='pending'").bind(authority.room_id()).bind(&proof.owner.participant_id).fetch_one(&mut *tx).await?;
        let recent: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM delegated_action_requests WHERE room_id=? AND owner_id=? AND created_at>?").bind(authority.room_id()).bind(&proof.owner.participant_id).bind((now - chrono::Duration::minutes(1)).timestamp_millis()).fetch_one(&mut *tx).await?;
        let room_pending: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM delegated_action_requests WHERE room_id=? AND state='pending'",
        )
        .bind(authority.room_id())
        .fetch_one(&mut *tx)
        .await?;
        if pending >= 4 || room_pending >= 16 || recent >= 8 {
            return Ok((
                json!({"status":"rejected","code":"rate_limited","message":"Owner confirmation limit reached.","retryable":true,"retry_after_ms":60000}),
                None,
            ));
        }
        let item = DelegatedActionRequest {
            confirmation_id: Uuid::new_v4().to_string(),
            owner_participant_id: proof.owner.participant_id.clone(), owner_display_name: proof.owner.display_name.clone(),
            delegate_participant_id: proof.delegate.participant_id.clone(), delegate_display_name: proof.delegate.display_name.clone(),
            action, target_name: target.name.clone(),
            consequences: "Deletes the channel and its message text, pins and search records. Event sequence and actor records are retained.".into(),
            created_at: now, expires_at: expiry, state: ConfirmationState::Pending,
        };
        let binding = Binding {
            room_uid: room.room_uid,
            authority: authority.clone(),
            owner_reference,
        };
        let encoded = serde_json::to_string(&item)?;
        let bytes = crate::room_write_budget::command_size(
            &request.request_id,
            "request_delete_text_channel",
            &serde_json::to_value(request)?,
        )?;
        crate::room_write_budget::reserve_room_write_budget(&mut tx, authority.room_id(), bytes)
            .await?;
        let debit = admit(
            &proof.owner.principal_id,
            &proof.delegate.principal_id,
            bytes,
        )?;
        sqlx::query("INSERT INTO delegated_action_requests(room_id,confirmation_id,owner_id,delegate_id,request_id,created_at,expires_at,state,binding_json,request_json) VALUES(?,?,?,?,?,?,?,'pending',?,?)")
            .bind(authority.room_id()).bind(&item.confirmation_id).bind(&item.owner_participant_id).bind(&item.delegate_participant_id).bind(&request.request_id)
            .bind(now.timestamp_millis()).bind(expiry.timestamp_millis()).bind(serde_json::to_string(&binding)?).bind(encoded).execute(&mut *tx).await?;
        crate::delegated_confirmation_lifecycle::event_in(
            &mut tx,
            authority.room_id(),
            &item,
            true,
        )
        .await?;
        tx.commit().await?;
        Ok((
            json!({"status":"pending_confirmation","request_id":request.request_id,"confirmation_id":item.confirmation_id,"expires_at":expiry}),
            Some(debit),
        ))
    }
}
