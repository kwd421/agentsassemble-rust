use agentsassemble_domain::AuthenticatedPrincipal;
use chrono::{DateTime, Duration, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    LocalRoomManagerAuthority, PersistenceError, RoomUserIdentity, SqliteStore,
    room_user_identity::resolve_local_room_manager,
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

const PAIRING_TTL: Duration = Duration::seconds(120);
const SESSION_TTL: Duration = Duration::hours(1);
pub(crate) const NATIVE_IDLE_TTL: Duration = Duration::days(30);
pub(crate) const ACTIVITY_WRITE_INTERVAL_SECONDS: i64 = 60;
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
    record_operator_use(tx, &current, Utc::now()).await?;
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

/// Exact inputs for one centrally authorized room session.
pub struct CentralOwnerSessionRequest<'a> {
    room_id: &'a str,
    room_incarnation: Uuid,
    grant_fingerprint: &'a [u8; 32],
    device_fingerprint: &'a [u8; 32],
    target_origin: &'a str,
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
}

impl<'a> CentralOwnerSessionRequest<'a> {
    /// Captures the exact room incarnation and remote browser authority for one grant.
    #[must_use]
    pub const fn new(
        room_id: &'a str,
        room_incarnation: Uuid,
        grant_fingerprint: &'a [u8; 32],
        device_fingerprint: &'a [u8; 32],
        target_origin: &'a str,
        expires_at: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            room_id,
            room_incarnation,
            grant_fingerprint,
            device_fingerprint,
            target_origin,
            expires_at: Some(expires_at),
            now,
        }
    }

    #[must_use]
    pub const fn host_owned(
        room_id: &'a str,
        room_incarnation: Uuid,
        grant_fingerprint: &'a [u8; 32],
        device_fingerprint: &'a [u8; 32],
        target_origin: &'a str,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            room_id,
            room_incarnation,
            grant_fingerprint,
            device_fingerprint,
            target_origin,
            expires_at: None,
            now,
        }
    }
}

/// Persistence-issued provenance for one room-scoped operator session.
#[derive(Clone)]
pub struct OperatorSessionAuthorization {
    session_fingerprint: [u8; 32],
    device_fingerprint: [u8; 32],
    target_origin: String,
    principal: AuthenticatedPrincipal,
    central_owner: bool,
    owner_session_fingerprint: Option<[u8; 32]>,
    expires_at: Option<DateTime<Utc>>,
}

impl OperatorSessionAuthorization {
    #[must_use]
    pub const fn owner_session_fingerprint(&self) -> Option<&[u8; 32]> {
        self.owner_session_fingerprint.as_ref()
    }
    /// True only for provenance minted by verified central-owner admission.
    #[must_use]
    pub const fn is_central_owner(&self) -> bool {
        self.central_owner
    }

    #[must_use]
    pub const fn session_fingerprint(&self) -> &[u8; 32] {
        &self.session_fingerprint
    }

    #[must_use]
    pub const fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }

    #[must_use]
    pub const fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }
}

impl SqliteStore {
    /// Resolves the renewable parent of a currently admitted room session.
    /// # Errors
    /// Rejects changed/expired room or parent authority.
    pub async fn owner_for_operator_session(
        &self,
        expected: &OperatorSessionAuthorization,
    ) -> Result<Option<crate::OwnerSessionAuthorization>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = revalidate_operator_session(&mut tx, expected, Utc::now()).await?;
        if !current.is_central_owner() {
            tx.commit().await?;
            return Ok(None);
        }
        let owner = match current.owner_session_fingerprint {
            Some(parent) => Some(
                crate::host_owner_session::resolve(
                    &mut tx,
                    &parent,
                    &current.device_fingerprint,
                    &current.target_origin,
                )
                .await?,
            ),
            None => None,
        };
        tx.commit().await?;
        Ok(owner)
    }

    /// Creates or replays one short room session authorized by a central-owner grant.
    ///
    /// # Errors
    /// Rejects a stale room, foreign replay, excessive expiry, capacity, or storage failure.
    pub async fn create_central_owner_session(
        &self,
        owner: &crate::ServerOwnerAuthority,
        request: &CentralOwnerSessionRequest<'_>,
    ) -> Result<OperatorPairingRedemption, PersistenceError> {
        require_origin(request.target_origin)?;
        let parent = central_session_parent(owner, request)?;
        if !matches!((parent, request.expires_at), (Some(_), None))
            && !matches!((parent, request.expires_at), (None, Some(expiry)) if expiry > request.now && expiry <= request.now + Duration::minutes(5))
        {
            return Err(unavailable());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        owner.revalidate(&mut tx).await?;
        let (manager, principal) = resolve_local_room_manager(
            &mut tx,
            request.room_id,
            agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
        if manager.room_uid != request.room_incarnation {
            return Err(unavailable());
        }
        let issued = derive_session_bearer(
            self.host_key.session_hmac_key(),
            request.grant_fingerprint,
            SessionBearerPurpose::OperatorPairing,
        );
        let redemption = OperatorPairingRedemption {
            session_bearer: issued.bearer,
            authorization: OperatorSessionAuthorization {
                session_fingerprint: issued.fingerprint,
                device_fingerprint: *request.device_fingerprint,
                target_origin: request.target_origin.to_owned(),
                principal,
                central_owner: true,
                owner_session_fingerprint: parent,
                expires_at: request.expires_at,
            },
        };
        if let Some(row) =
            sqlx::query("SELECT * FROM operator_pairings WHERE token_fingerprint = ?")
                .bind(request.grant_fingerprint.as_slice())
                .fetch_optional(&mut *tx)
                .await?
        {
            let record = PairingRecord::decode(&row)?;
            record.require_origin_and_live(request.target_origin)?;
            if !record.central_owner
                || record.owner_session_fingerprint != parent
                || record.manager != manager
                || record.device_fingerprint.as_ref() != Some(request.device_fingerprint)
                || record.session_fingerprint.as_ref() != Some(&issued.fingerprint)
                || record.require_session(&issued.fingerprint, request.now)? != request.expires_at
            {
                return Err(unavailable());
            }
            tx.commit().await?;
            return Ok(redemption);
        }
        cleanup_pairings(&mut tx, request.now).await?;
        let (total, room): (i64, i64) =
            sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(room_id = ?), 0) FROM operator_pairings")
                .bind(request.room_id)
                .fetch_one(&mut *tx)
                .await?;
        if total >= MAX_PAIRINGS || room >= MAX_ROOM_PAIRINGS {
            return Err(rejected(
                "pairing_capacity",
                "End an existing pairing or wait for expiry.",
            ));
        }
        sqlx::query(concat!(
            "INSERT INTO operator_pairings (pairing_id, token_fingerprint, room_id, room_uid, ",
            "server_id, authority_lineage_id, user_id, participant_id, target_origin, expires_at, ",
            "device_fingerprint, session_fingerprint, session_expires_at, central_owner, host_owner_session_fingerprint) ",
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?)"
        ))
        .bind(Uuid::new_v4().to_string())
        .bind(request.grant_fingerprint.as_slice())
        .bind(&manager.manager.room_id)
        .bind(manager.room_uid.to_string())
        .bind(&manager.server_id)
        .bind(&manager.authority_lineage_id)
        .bind(&manager.manager.user_id)
        .bind(&manager.manager.participant_id)
        .bind(request.target_origin)
        .bind(request.expires_at.unwrap_or(request.now + PAIRING_TTL).timestamp_micros())
        .bind(request.device_fingerprint.as_slice())
        .bind(issued.fingerprint.as_slice())
        // Zero denotes no clock expiry only for a verified host-owned parent.
        .bind(request.expires_at.map_or(0, |expiry| expiry.timestamp_micros()))
        .bind(parent.as_ref().map(<[u8; 32]>::as_slice))
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(redemption)
    }

    /// Creates a bounded grant for an exact current room manager and ready ingress origin.
    ///
    /// # Errors
    /// Rejects stale manager authority, invalid origin, capacity exhaustion and storage failures.
    pub async fn create_operator_pairing(
        &self,
        authority: &crate::RoomManagerAuthority,
        token_fingerprint: &[u8; 32],
        target_origin: &str,
        now: DateTime<Utc>,
    ) -> Result<OperatorPairing, PersistenceError> {
        let now = timestamp(now.timestamp_micros())?;
        require_origin(target_origin)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let manager = authority.exact_binding(&mut tx).await?;
        let parent = match authority {
            crate::RoomManagerAuthority::Operator(session) => session.owner_session_fingerprint,
            crate::RoomManagerAuthority::Local(_) => None,
        };
        cleanup_pairings(&mut tx, now).await?;
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
            "server_id, authority_lineage_id, user_id, participant_id, target_origin, expires_at, host_owner_session_fingerprint) ",
            "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"
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
        .bind(parent.as_ref().map(<[u8; 32]>::as_slice))
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
            let expires_at = if record.native_issued() {
                None
            } else {
                Some(now + SESSION_TTL)
            };
            sqlx::query(concat!(
                "UPDATE operator_pairings SET device_fingerprint = ?, ",
                "session_fingerprint = ?, session_expires_at = ?, last_connected_at = ? WHERE pairing_id = ?"
            ))
            .bind(device_fingerprint.as_slice())
            .bind(issued.fingerprint.as_slice())
            .bind(expires_at.map_or(0, |expiry| expiry.timestamp_micros()))
            .bind(now.timestamp())
            .bind(&record.pairing_id)
            .execute(&mut *tx)
            .await?;
            expires_at
        };
        if let Some(parent) = record.owner_session_fingerprint {
            crate::host_owner_session::require_unrevoked_issuer(&mut tx, &parent, target_origin)
                .await?;
        }
        record.record_use(&mut tx, now).await?;
        tx.commit().await?;
        Ok(OperatorPairingRedemption {
            session_bearer: issued.bearer,
            authorization: OperatorSessionAuthorization {
                session_fingerprint: issued.fingerprint,
                device_fingerprint: *device_fingerprint,
                target_origin: target_origin.to_owned(),
                principal,
                central_owner: record.central_owner,
                owner_session_fingerprint: record.owner_session_fingerprint,
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
    /// Rejects stale manager authority, foreign pairings and storage failures.
    pub async fn revoke_operator_pairing(
        &self,
        authority: &crate::RoomManagerAuthority,
        pairing_id: Uuid,
    ) -> Result<Option<[u8; 32]>, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let manager = authority.exact_binding(&mut tx).await?;
        let row = sqlx::query("SELECT * FROM operator_pairings WHERE pairing_id = ?")
            .bind(pairing_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let record = PairingRecord::decode(&row)?;
        if record.manager != manager {
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

async fn cleanup_pairings(
    tx: &mut Transaction<'_, Sqlite>,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    sqlx::query("DELETE FROM operator_pairings WHERE (COALESCE(session_expires_at, expires_at) > 0 AND COALESCE(session_expires_at, expires_at) <= ?) OR (central_owner = 0 AND host_owner_session_fingerprint IS NULL AND session_expires_at = 0 AND last_connected_at <= ?) OR (central_owner = 1 AND host_owner_session_fingerprint IS NOT NULL AND EXISTS (SELECT 1 FROM host_owner_sessions WHERE fingerprint = operator_pairings.host_owner_session_fingerprint AND (connected = 0 OR revoked = 1)))")
        .bind(now.timestamp_micros()).bind((now - NATIVE_IDLE_TTL).timestamp()).execute(&mut **tx).await?;
    Ok(())
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
        || current.central_owner != expected.central_owner
        || current.owner_session_fingerprint != expected.owner_session_fingerprint
    {
        return Err(unavailable());
    }
    Ok(current)
}

pub(crate) async fn revalidate_central_owner_session(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &OperatorSessionAuthorization,
) -> Result<OperatorSessionAuthorization, PersistenceError> {
    let current = revalidate_operator_session(tx, expected, Utc::now()).await?;
    if !current.is_central_owner() {
        return Err(unavailable());
    }
    Ok(current)
}

pub(crate) async fn record_operator_use(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &OperatorSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    // Admission or exact durable-effect custody was validated in this transaction.
    // The UPDATE excludes revoked/idle-expired credentials even during effect completion.
    session_record(tx, expected.session_fingerprint())
        .await?
        .record_use(tx, now)
        .await
}

async fn resolve_operator_session(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    device: &[u8; 32],
    origin: &str,
    now: DateTime<Utc>,
) -> Result<OperatorSessionAuthorization, PersistenceError> {
    let record = session_record(tx, fingerprint).await?;
    record.require_origin_and_live(origin)?;
    if record.device_fingerprint.as_ref() != Some(device) {
        return Err(unavailable());
    }
    let expires_at = record.require_session(fingerprint, now)?;
    if let Some(parent) = record.owner_session_fingerprint {
        if record.central_owner {
            crate::host_owner_session::resolve(tx, &parent, device, origin).await?;
            if expires_at.is_some() {
                return Err(unavailable());
            }
        } else {
            crate::host_owner_session::require_unrevoked_issuer(tx, &parent, origin).await?;
        }
    }
    let principal = record.resolve_manager(tx).await?;
    Ok(OperatorSessionAuthorization {
        session_fingerprint: *fingerprint,
        device_fingerprint: *device,
        target_origin: origin.to_owned(),
        principal,
        central_owner: record.central_owner,
        owner_session_fingerprint: record.owner_session_fingerprint,
        expires_at,
    })
}

// Validates the stored parent of an attendee, never a presented operator credential.
pub(crate) async fn require_attendee_parent(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    room_id: &str,
    now: DateTime<Utc>,
) -> Result<(AuthenticatedPrincipal, Option<DateTime<Utc>>), PersistenceError> {
    let record = session_record(tx, fingerprint).await?;
    if record.revoked || record.manager.manager.room_id != room_id {
        return Err(unavailable());
    }
    let expires_at = record.require_session(fingerprint, now)?;
    if let Some(parent) = record.owner_session_fingerprint {
        if record.central_owner {
            let device = record.device_fingerprint.as_ref().ok_or_else(unavailable)?;
            crate::host_owner_session::resolve(tx, &parent, device, &record.target_origin).await?;
        } else {
            crate::host_owner_session::require_unrevoked_issuer(tx, &parent, &record.target_origin)
                .await?;
        }
    }
    let principal = record.resolve_manager(tx).await?;
    Ok((principal, expires_at))
}

async fn session_record(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<PairingRecord, PersistenceError> {
    let row = sqlx::query("SELECT * FROM operator_pairings WHERE session_fingerprint = ?")
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(unavailable)?;
    PairingRecord::decode(&row)
}

struct PairingRecord {
    pairing_id: String,
    central_owner: bool,
    owner_session_fingerprint: Option<[u8; 32]>,
    manager: LocalRoomManagerAuthority,
    target_origin: String,
    expires_at: DateTime<Utc>,
    revoked: bool,
    device_fingerprint: Option<[u8; 32]>,
    session_fingerprint: Option<[u8; 32]>,
    session_expires_at: Option<DateTime<Utc>>,
    last_connected_at: Option<i64>,
}

impl PairingRecord {
    fn decode(row: &sqlx::sqlite::SqliteRow) -> Result<Self, PersistenceError> {
        let room_uid: String = row.try_get("room_uid")?;
        Ok(Self {
            pairing_id: row.try_get("pairing_id")?,
            central_owner: row.try_get("central_owner")?,
            owner_session_fingerprint: optional_fingerprint(
                row.try_get("host_owner_session_fingerprint")?,
            )?,
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
                .filter(|value| *value != 0)
                .map(timestamp)
                .transpose()?,
            last_connected_at: row.try_get("last_connected_at")?,
        })
    }

    fn native_issued(&self) -> bool {
        !self.central_owner && self.owner_session_fingerprint.is_none()
    }

    async fn record_use(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        now: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        if self.native_issued() {
            sqlx::query("UPDATE operator_pairings SET last_connected_at = ? WHERE pairing_id = ? AND revoked = 0 AND last_connected_at > ? AND last_connected_at < ?")
                .bind(now.timestamp()).bind(&self.pairing_id).bind((now - NATIVE_IDLE_TTL).timestamp()).bind(now.timestamp() - ACTIVITY_WRITE_INTERVAL_SECONDS)
                .execute(&mut **tx).await?;
        }
        Ok(())
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
    ) -> Result<Option<DateTime<Utc>>, PersistenceError> {
        let expires = self.session_expires_at;
        if self.session_fingerprint.as_ref() != Some(fingerprint)
            || expires.is_some_and(|value| value <= now)
            || (expires.is_none()
                && !(self.central_owner && self.owner_session_fingerprint.is_some())
                && !(self.native_issued()
                    && self
                        .last_connected_at
                        .is_some_and(|last| last > (now - NATIVE_IDLE_TTL).timestamp())))
        {
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

pub(crate) fn require_origin(origin: &str) -> Result<(), PersistenceError> {
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

fn central_session_parent(
    owner: &crate::ServerOwnerAuthority,
    request: &CentralOwnerSessionRequest<'_>,
) -> Result<Option<[u8; 32]>, PersistenceError> {
    match owner {
        crate::ServerOwnerAuthority::CentralSession(session) => {
            let binding = session.binding();
            if binding.browser_fingerprint != *request.device_fingerprint
                || binding.origin != request.target_origin
                || request.expires_at.is_some()
            {
                return Err(unavailable());
            }
            Ok(Some(*session.fingerprint()))
        }
        _ => Ok(None),
    }
}
