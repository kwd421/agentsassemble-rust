use agentsassemble_domain::AuthenticatedPrincipal;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    LocalRoomManagerAuthority, PersistenceError, RoomUserIdentity, SqliteStore,
    room_user_identity::{require_exact_local_room_manager, resolve_local_room_manager},
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

const PAIRING_TTL: Duration = Duration::seconds(120);
const SESSION_TTL: Duration = Duration::hours(1);
const MAX_PAIRINGS: i64 = 128;
const MAX_ROOM_PAIRINGS: i64 = 32;

pub(crate) async fn leave_operator_session(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &OperatorSessionAuthorization,
    request_id: &str,
    payload: &serde_json::Value,
) -> Result<crate::ParticipantLeaveMutation, PersistenceError> {
    let current = revalidate_operator_session(tx, expected, Utc::now()).await?;
    let principal = current.principal();
    let hash = agentsassemble_domain::canonical_payload_hash(payload);
    if crate::command_admission::inspect_non_lifecycle_command(
        tx,
        &principal.room_id,
        &principal.principal_id,
        request_id,
        crate::participant_leave::PARTICIPANT_LEAVE_ACTION,
        &hash,
    )
    .await?
    .is_some()
    {
        return Err(PersistenceError::CommandConflict);
    }
    sqlx::query("UPDATE operator_pairings SET revoked = 1 WHERE session_fingerprint = ?")
        .bind(current.session_fingerprint().as_slice())
        .execute(&mut **tx)
        .await?;
    let event = agentsassemble_domain::RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(tx, &principal.room_id).await?,
        created_at: Utc::now(),
        room_id: principal.room_id.clone(),
        event_type: "operator_session_ended".to_owned(),
        actor: agentsassemble_domain::Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: None,
        participant_type: None,
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: None,
        content: None,
        message_kind: None,
        extra: std::collections::BTreeMap::new(),
    };
    crate::room_turns::support::insert_event(tx, &event).await?;
    let outcome = crate::agent_lifecycle_events::store_result(
        tx,
        principal,
        request_id,
        crate::participant_leave::PARTICIPANT_LEAVE_ACTION,
        hash,
        serde_json::json!({"status": "left", "participant_id": principal.participant_id, "event": event, "event_seq": event.seq}),
        vec![event],
    )
    .await?;
    Ok(crate::ParticipantLeaveMutation {
        outcome,
        revoked_session_fingerprints: vec![*current.session_fingerprint()],
    })
}

/// One short-lived grant; its secret token is held only by the transport issuer.
#[derive(Clone)]
pub struct OperatorPairing {
    pub pairing_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

/// A response containing secret material; deliberately not serializable or printable.
pub struct OperatorPairingRedemption {
    pub session_bearer: String,
    pub authorization: OperatorSessionAuthorization,
}

/// Persistence-issued provenance for one room-scoped operator session.
#[derive(Clone)]
pub struct OperatorSessionAuthorization {
    session_fingerprint: [u8; 32],
    device_fingerprint: [u8; 32],
    target_origin: String,
    principal: AuthenticatedPrincipal,
    expires_at: DateTime<Utc>,
}

impl OperatorSessionAuthorization {
    #[must_use]
    pub const fn session_fingerprint(&self) -> &[u8; 32] {
        &self.session_fingerprint
    }

    #[must_use]
    pub const fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

impl SqliteStore {
    /// Creates a bounded grant for an exact local manager and ready ingress origin.
    ///
    /// # Errors
    /// Rejects stale manager authority, invalid origin, capacity exhaustion and storage failures.
    pub async fn create_operator_pairing(
        &self,
        manager: &LocalRoomManagerAuthority,
        token_fingerprint: &[u8; 32],
        target_origin: &str,
        now: DateTime<Utc>,
    ) -> Result<OperatorPairing, PersistenceError> {
        let now = timestamp(now.timestamp_micros())?;
        require_origin(target_origin)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_exact_local_room_manager(&mut tx, manager).await?;
        sqlx::query(
            "DELETE FROM operator_pairings WHERE COALESCE(session_expires_at, expires_at) <= ?",
        )
        .bind(now.timestamp_micros())
        .execute(&mut *tx)
        .await?;
        let (total, room): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(room_id = ?), 0) FROM operator_pairings")
                .bind(&manager.manager.room_id)
                .fetch_one(&mut *tx)
                .await?;
        if total >= MAX_PAIRINGS || room >= MAX_ROOM_PAIRINGS {
            return Err(rejected(
                "pairing_capacity",
                "End an existing pairing or wait for expiry.",
            ));
        }
        let result = OperatorPairing {
            pairing_id: Uuid::new_v4(),
            expires_at: now + PAIRING_TTL,
        };
        sqlx::query(concat!(
            "INSERT INTO operator_pairings (pairing_id, token_fingerprint, room_id, room_uid, ",
            "server_id, authority_lineage_id, user_id, participant_id, target_origin, expires_at) ",
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
        ))
        .bind(result.pairing_id.to_string())
        .bind(token_fingerprint.as_slice())
        .bind(&manager.manager.room_id)
        .bind(manager.room_uid.to_string())
        .bind(&manager.server_id)
        .bind(&manager.authority_lineage_id)
        .bind(&manager.manager.user_id)
        .bind(&manager.manager.participant_id)
        .bind(target_origin)
        .bind(result.expires_at.timestamp_micros())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Atomically consumes a pairing or returns its still-live same-device result.
    ///
    /// # Errors
    /// Rejects expired/revoked grants, wrong device/origin, stale host/room and storage failures.
    pub async fn redeem_operator_pairing(
        &self,
        token_fingerprint: &[u8; 32],
        device_fingerprint: &[u8; 32],
        target_origin: &str,
        now: DateTime<Utc>,
    ) -> Result<OperatorPairingRedemption, PersistenceError> {
        let now = timestamp(now.timestamp_micros())?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM operator_pairings WHERE token_fingerprint = ?")
            .bind(token_fingerprint.as_slice())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(unavailable)?;
        let record = PairingRecord::decode(&row)?;
        record.require_origin_and_live(target_origin)?;
        let principal = record.resolve_manager(&mut tx).await?;
        let issued = derive_session_bearer(
            self.host_key.session_hmac_key(),
            token_fingerprint,
            SessionBearerPurpose::OperatorPairing,
        );
        let expires_at = if let Some(device) = record.device_fingerprint {
            if device != *device_fingerprint {
                return Err(rejected(
                    "pairing_already_used",
                    "This pairing was used by another device.",
                ));
            }
            record.require_session(&issued.fingerprint, now)?
        } else {
            if record.expires_at <= now {
                return Err(unavailable());
            }
            let expires_at = now + SESSION_TTL;
            sqlx::query(concat!(
                "UPDATE operator_pairings SET device_fingerprint = ?, ",
                "session_fingerprint = ?, session_expires_at = ? WHERE pairing_id = ?"
            ))
            .bind(device_fingerprint.as_slice())
            .bind(issued.fingerprint.as_slice())
            .bind(expires_at.timestamp_micros())
            .bind(&record.pairing_id)
            .execute(&mut *tx)
            .await?;
            expires_at
        };
        tx.commit().await?;
        Ok(OperatorPairingRedemption {
            session_bearer: issued.bearer,
            authorization: OperatorSessionAuthorization {
                session_fingerprint: issued.fingerprint,
                device_fingerprint: *device_fingerprint,
                target_origin: target_origin.to_owned(),
                principal,
                expires_at,
            },
        })
    }

    /// Resolves a paired session without creating durable account or local host authority.
    ///
    /// # Errors
    /// Rejects a missing, expired, revoked, foreign-device or foreign-origin session.
    pub async fn authorize_operator_session(
        &self,
        session_fingerprint: &[u8; 32],
        device_fingerprint: &[u8; 32],
        target_origin: &str,
    ) -> Result<OperatorSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let authorization = resolve_operator_session(
            &mut tx,
            session_fingerprint,
            device_fingerprint,
            target_origin,
            Utc::now(),
        )
        .await?;
        tx.commit().await?;
        Ok(authorization)
    }

    /// Revokes exactly one grant and returns its session fingerprint for publication after commit.
    ///
    /// # Errors
    /// Rejects stale local manager authority, foreign pairings and storage failures.
    pub async fn revoke_operator_pairing(
        &self,
        manager: &LocalRoomManagerAuthority,
        pairing_id: Uuid,
    ) -> Result<Option<[u8; 32]>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_exact_local_room_manager(&mut tx, manager).await?;
        let row = sqlx::query("SELECT * FROM operator_pairings WHERE pairing_id = ?")
            .bind(pairing_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let record = PairingRecord::decode(&row)?;
        if record.manager != *manager {
            return Err(unavailable());
        }
        sqlx::query("UPDATE operator_pairings SET revoked = 1 WHERE pairing_id = ?")
            .bind(pairing_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(record.session_fingerprint)
    }
}

pub(crate) async fn revalidate_operator_session(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &OperatorSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<OperatorSessionAuthorization, PersistenceError> {
    let current = resolve_operator_session(
        tx,
        &expected.session_fingerprint,
        &expected.device_fingerprint,
        &expected.target_origin,
        now,
    )
    .await?;
    if current.principal.room_id != expected.principal.room_id
        || current.expires_at != expected.expires_at
    {
        return Err(unavailable());
    }
    Ok(current)
}

async fn resolve_operator_session(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    device: &[u8; 32],
    origin: &str,
    now: DateTime<Utc>,
) -> Result<OperatorSessionAuthorization, PersistenceError> {
    let row = sqlx::query("SELECT * FROM operator_pairings WHERE session_fingerprint = ?")
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(unavailable)?;
    let record = PairingRecord::decode(&row)?;
    record.require_origin_and_live(origin)?;
    if record.device_fingerprint.as_ref() != Some(device) {
        return Err(unavailable());
    }
    let expires_at = record.require_session(fingerprint, now)?;
    let principal = record.resolve_manager(tx).await?;
    Ok(OperatorSessionAuthorization {
        session_fingerprint: *fingerprint,
        device_fingerprint: *device,
        target_origin: origin.to_owned(),
        principal,
        expires_at,
    })
}

struct PairingRecord {
    pairing_id: String,
    manager: LocalRoomManagerAuthority,
    target_origin: String,
    expires_at: DateTime<Utc>,
    revoked: bool,
    device_fingerprint: Option<[u8; 32]>,
    session_fingerprint: Option<[u8; 32]>,
    session_expires_at: Option<DateTime<Utc>>,
}

impl PairingRecord {
    fn decode(row: &sqlx::sqlite::SqliteRow) -> Result<Self, PersistenceError> {
        let room_uid: String = row.try_get("room_uid")?;
        Ok(Self {
            pairing_id: row.try_get("pairing_id")?,
            manager: LocalRoomManagerAuthority {
                server_id: row.try_get("server_id")?,
                authority_lineage_id: row.try_get("authority_lineage_id")?,
                room_uid: Uuid::parse_str(&room_uid).map_err(|_| invalid_state())?,
                manager: RoomUserIdentity {
                    room_id: row.try_get("room_id")?,
                    user_id: row.try_get("user_id")?,
                    participant_id: row.try_get("participant_id")?,
                },
            },
            target_origin: row.try_get("target_origin")?,
            expires_at: timestamp(row.try_get("expires_at")?)?,
            revoked: row.try_get("revoked")?,
            device_fingerprint: optional_fingerprint(row.try_get("device_fingerprint")?)?,
            session_fingerprint: optional_fingerprint(row.try_get("session_fingerprint")?)?,
            session_expires_at: row
                .try_get::<Option<i64>, _>("session_expires_at")?
                .map(timestamp)
                .transpose()?,
        })
    }

    fn require_origin_and_live(&self, origin: &str) -> Result<(), PersistenceError> {
        if self.revoked || self.target_origin != origin {
            return Err(unavailable());
        }
        Ok(())
    }

    fn require_session(
        &self,
        fingerprint: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<DateTime<Utc>, PersistenceError> {
        let expires = self.session_expires_at.ok_or_else(invalid_state)?;
        if self.session_fingerprint.as_ref() != Some(fingerprint) || expires <= now {
            return Err(unavailable());
        }
        Ok(expires)
    }

    async fn resolve_manager(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<AuthenticatedPrincipal, PersistenceError> {
        let (current, principal) = resolve_local_room_manager(
            tx,
            &self.manager.manager.room_id,
            &self.manager.manager.user_id,
            &self.manager.manager.participant_id,
        )
        .await?;
        if current != self.manager {
            return Err(unavailable());
        }
        Ok(principal)
    }
}

fn require_origin(origin: &str) -> Result<(), PersistenceError> {
    let parsed = url::Url::parse(origin)
        .map_err(|_| rejected("invalid_origin", "Invalid pairing origin."))?;
    if parsed.scheme() != "https" || parsed.origin().ascii_serialization() != origin {
        return Err(rejected(
            "invalid_origin",
            "Pairing requires a canonical HTTPS origin.",
        ));
    }
    Ok(())
}

fn optional_fingerprint(value: Option<Vec<u8>>) -> Result<Option<[u8; 32]>, PersistenceError> {
    value
        .map(|bytes| bytes.try_into().map_err(|_| invalid_state()))
        .transpose()
}

fn timestamp(value: i64) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_micros(value).ok_or_else(invalid_state)
}

fn invalid_state() -> PersistenceError {
    rejected("invalid_state", "Stored operator pairing is invalid.")
}

fn unavailable() -> PersistenceError {
    rejected(
        "session_revoked",
        "This operator pairing or session is no longer available.",
    )
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

#[cfg(test)]
#[path = "operator_pairing_tests.rs"]
mod tests;
