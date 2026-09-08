//! Claims one live response while persisting only its secret-free projection.
use std::collections::BTreeMap;

use agentsassemble_domain::{ProviderRequest, ProviderRequestResolution, RoomEvent};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde_json::json;
use sha2::Sha256;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, RoomMutationAuthority, SqliteStore,
    provider_requests::rejected,
    room_turns::support::{internal_event, load_event},
};

/// Issued once after the transaction wins. Never serialized or reconstructed from storage.
pub struct ProviderRequestDelivery {
    pub(crate) room_id: String,
    pub(crate) session_id: String,
    pub(crate) request_id: Uuid,
    pub(crate) turn_generation: u64,
    pub(crate) execution_id: String,
    pub(crate) fingerprint: [u8; 32],
    pub(crate) resolution: ProviderRequestResolution,
}

impl ProviderRequestDelivery {
    #[must_use]
    pub fn target(&self) -> (&str, &str, Uuid, u64, &str) {
        (
            &self.room_id,
            &self.session_id,
            self.request_id,
            self.turn_generation,
            &self.execution_id,
        )
    }

    #[must_use]
    pub const fn resolution(&self) -> &ProviderRequestResolution {
        &self.resolution
    }
}

pub struct ProviderRequestResolutionCommit {
    pub event: RoomEvent,
    pub delivery: Option<ProviderRequestDelivery>,
}

impl SqliteStore {
    /// Claims live delivery only for the current human owner and exact provider execution.
    ///
    /// # Errors
    /// Rejects changed answers, stale owner/runtime/connection authority and closed requests.
    pub async fn resolve_provider_request(
        &self,
        authority: RoomMutationAuthority<'_>,
        request_id: Uuid,
        resolution: &ProviderRequestResolution,
        now: DateTime<Utc>,
    ) -> Result<ProviderRequestResolutionCommit, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let principal = authority.resolve(&mut tx).await?;
        let row = sqlx::query("SELECT * FROM provider_requests WHERE room_id=? AND request_id=?")
            .bind(&principal.room_id)
            .bind(request_id.to_string())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                rejected(
                    "provider_request_missing",
                    "Provider request is unavailable.",
                )
            })?;
        let session = super::provider_request_authority::authorize_resolution_in(
            &mut tx, &principal, &row, now,
        )
        .await?;
        let request: ProviderRequest = serde_json::from_str(row.get("request_json"))?;
        let durable = request.durable_resolution(resolution).ok_or_else(|| {
            rejected(
                "invalid_provider_response",
                "Response does not match the provider request.",
            )
        })?;
        let fingerprint = response_fingerprint(
            self.host_key.session_hmac_key(),
            &principal.room_id,
            request_id,
            resolution,
        )?;
        let previous: Option<Vec<u8>> = row.get("resolution_fingerprint");
        if let Some(previous) = previous {
            if previous != fingerprint {
                return Err(PersistenceError::CommandConflict);
            }
            let event =
                resolution_event_in(&mut tx, &principal.room_id, row.get("resolution_event_id"))
                    .await?;
            tx.commit().await?;
            return Ok(ProviderRequestResolutionCommit {
                event,
                delivery: None,
            });
        }
        if row.get::<&str, _>("state") != "open"
            || row.get::<i64, _>("expires_at") <= now.timestamp_millis()
        {
            return Err(rejected(
                "provider_request_closed",
                "Provider request is no longer open.",
            ));
        }
        let encoded = serde_json::to_string(&durable)?;
        crate::room_write_budget::reserve_room_write_budget(
            &mut tx,
            &principal.room_id,
            encoded.len(),
        )
        .await?;
        let event = internal_event(
            &mut tx,
            &session,
            "provider_request_resolving",
            false,
            None,
            BTreeMap::from([
                ("visibility".to_owned(), json!("owner")),
                ("owner_id".to_owned(), json!(principal.participant_id)),
                ("provider_request_id".to_owned(), json!(request_id)),
                ("resolution".to_owned(), json!(durable)),
            ]),
        )
        .await?;
        sqlx::query("UPDATE provider_requests SET state='resolving', resolution_json=?, resolution_fingerprint=?, resolution_event_id=? WHERE room_id=? AND request_id=?")
            .bind(encoded).bind(fingerprint.as_slice()).bind(&event.id)
            .bind(&principal.room_id).bind(request_id.to_string())
            .execute(&mut *tx).await?;
        let delivery = ProviderRequestDelivery {
            room_id: principal.room_id.clone(),
            session_id: session.public.session_id,
            request_id,
            turn_generation: session.turn_generation,
            execution_id: row.get("execution_id"),
            fingerprint,
            resolution: resolution.clone(),
        };
        tx.commit().await?;
        Ok(ProviderRequestResolutionCommit {
            event,
            delivery: Some(delivery),
        })
    }
}

async fn resolution_event_in(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: Option<&str>,
) -> Result<RoomEvent, PersistenceError> {
    let id = event_id
        .ok_or_else(|| rejected("invalid_state", "Provider resolution event is missing."))?;
    load_event(tx, room_id, id)
        .await?
        .ok_or_else(|| rejected("invalid_state", "Provider resolution event is missing."))
}

fn response_fingerprint(
    key: &[u8; 32],
    room_id: &str,
    request_id: Uuid,
    resolution: &ProviderRequestResolution,
) -> Result<[u8; 32], PersistenceError> {
    // A raw hash would let a database reader guess low-entropy secret answers offline.
    // Purpose separation keeps this comparison independent from session bearer derivation.
    let mut signer = Hmac::<Sha256>::new_from_slice(key)
        .unwrap_or_else(|_| unreachable!("HMAC accepts a 32-byte key"));
    signer.update(b"agentsassemble-provider-response-v1\0");
    signer.update(&serde_json::to_vec(&(room_id, request_id, resolution))?);
    Ok(signer.finalize().into_bytes().into())
}
