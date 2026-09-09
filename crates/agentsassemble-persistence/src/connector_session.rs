use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, ParticipantStatus,
};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, SqliteStore,
    connector_admission::{parse_uuid, timestamp},
};

/// Persistence-issued current-session AI provenance; never a human or managed Agent Session.
#[derive(Clone)]
pub struct ConnectorSessionAuthorization {
    principal: AuthenticatedPrincipal,
    fingerprint: [u8; 32],
    room_uid: Uuid,
    expires_at: DateTime<Utc>,
}

impl ConnectorSessionAuthorization {
    #[must_use]
    pub const fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }
    #[must_use]
    pub const fn session_fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
    #[must_use]
    pub const fn room_uid(&self) -> Uuid {
        self.room_uid
    }
}

impl SqliteStore {
    /// Recovers only an exact committed leave under the original credential and room incarnation.
    /// The returned viewer is projection data, not active connector session authority.
    ///
    /// # Errors
    /// Rejects changed credentials/incarnations, conflicting requests, invalid payloads and storage failures.
    pub async fn completed_connector_leave(
        &self,
        fingerprint: &[u8; 32],
        request_id: &str,
        payload: &Value,
    ) -> Result<Option<(AuthenticatedPrincipal, crate::CommandOutcome)>, PersistenceError> {
        if !crate::participant_leave::participant_leave_payload_is_exact(payload) {
            return Err(rejected(
                "permission_denied",
                "Connector leave requires an empty payload.",
            ));
        }
        let mut tx = self.pool.begin().await?;
        let record = load_record_in(&mut tx, fingerprint).await?;
        let outcome = leave_receipt_in(&mut tx, &record.principal, request_id).await?;
        tx.commit().await?;
        Ok(outcome.map(|outcome| (record.principal, outcome)))
    }

    /// Authenticates one current connector session under its owning room incarnation.
    ///
    /// # Errors
    /// Rejects expired/revoked credentials, inactive/mismatched membership and storage failures.
    pub async fn authorize_connector_session(
        &self,
        fingerprint: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<ConnectorSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let result = authorize_in(&mut tx, fingerprint, now).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Revalidates previously issued connector provenance at a new operation boundary.
    ///
    /// # Errors
    /// Rejects replaced authority, inactive membership or storage failures.
    pub async fn revalidate_connector_session(
        &self,
        expected: &ConnectorSessionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<ConnectorSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let result = revalidate_in(&mut tx, expected, now).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Ends only this connector's room membership and credential, without a provider process.
    ///
    /// # Errors
    /// Rejects stale authority, conflicting requests and persistence failures before committing.
    pub async fn leave_connector_session(
        &self,
        expected: &ConnectorSessionAuthorization,
        request_id: &str,
    ) -> Result<crate::ParticipantLeaveMutation, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let record = load_record_in(&mut tx, &expected.fingerprint).await?;
        record.validate_identity(expected)?;
        if let Some(outcome) = leave_receipt_in(&mut tx, &record.principal, request_id).await? {
            tx.commit().await?;
            return Ok(crate::ParticipantLeaveMutation {
                outcome,
                revoked_session_fingerprints: Vec::new(),
            });
        }
        let current = record.authorize(&expected.fingerprint, Utc::now())?;
        let principal = current.principal();
        let payload = json!({});
        let payload_hash = agentsassemble_domain::canonical_payload_hash(&payload);
        let mut participant = crate::room_turns::support::load_participant(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        participant.status = ParticipantStatus::Left;
        participant.updated_at = Utc::now();
        crate::participant_rows::save_participant_exact(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
            &participant,
        )
        .await?;
        sqlx::query("UPDATE room_connector_invites SET revoked=1 WHERE session_fingerprint=?")
            .bind(current.fingerprint.as_slice())
            .execute(&mut *tx)
            .await?;
        let event = crate::participant_leave::participant_left_event(&mut tx, &participant).await?;
        crate::room_turns::support::insert_event(&mut tx, &event).await?;
        let outcome = crate::agent_lifecycle_events::store_result(
            &mut tx,
            principal,
            request_id,
            "participant.leave",
            payload_hash,
            json!({"participant":participant,"event":event,"event_seq":event.seq}),
            vec![event],
        )
        .await?;
        tx.commit().await?;
        Ok(crate::ParticipantLeaveMutation {
            outcome,
            revoked_session_fingerprints: vec![current.fingerprint],
        })
    }
}

pub(crate) async fn revalidate_in(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &ConnectorSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<ConnectorSessionAuthorization, PersistenceError> {
    let record = load_record_in(tx, &expected.fingerprint).await?;
    record.validate_identity(expected)?;
    record.authorize(&expected.fingerprint, now)
}

pub(super) async fn authorize_in(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<ConnectorSessionAuthorization, PersistenceError> {
    load_record_in(tx, fingerprint)
        .await?
        .authorize(fingerprint, now)
}

// Stored identity can identify a terminal receipt without granting active session authority.
struct ConnectorSessionRecord {
    principal: AuthenticatedPrincipal,
    room_uid: Uuid,
    expires_at: DateTime<Utc>,
    revoked: bool,
    status: ParticipantStatus,
}

impl ConnectorSessionRecord {
    fn validate_identity(
        &self,
        expected: &ConnectorSessionAuthorization,
    ) -> Result<(), PersistenceError> {
        if self.room_uid != expected.room_uid
            || self.principal.room_id != expected.principal.room_id
            || self.principal.participant_id != expected.principal.participant_id
            || self.expires_at != expected.expires_at
        {
            return Err(rejected(
                "session_revoked",
                "The connector's exact room authority changed.",
            ));
        }
        Ok(())
    }

    fn authorize(
        self,
        fingerprint: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<ConnectorSessionAuthorization, PersistenceError> {
        if self.revoked || self.expires_at <= now || self.status != ParticipantStatus::Joined {
            return Err(rejected(
                "session_revoked",
                "The connector session has ended.",
            ));
        }
        Ok(ConnectorSessionAuthorization {
            principal: self.principal,
            fingerprint: *fingerprint,
            room_uid: self.room_uid,
            expires_at: self.expires_at,
        })
    }
}

async fn load_record_in(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
) -> Result<ConnectorSessionRecord, PersistenceError> {
    let row=sqlx::query("SELECT room_id,room_uid,participant_id,scope,revoked,session_expires_at FROM room_connector_invites WHERE session_fingerprint=?")
        .bind(fingerprint.as_slice()).fetch_optional(&mut **tx).await?.ok_or_else(||rejected("session_revoked","The connector session is unavailable."))?;
    let expires_at = timestamp(row.get("session_expires_at"))?;
    let room_id: String = row.get("room_id");
    let participant_id: String = row.get("participant_id");
    let room = crate::authority::load_active_room(tx, &room_id).await?;
    let participant =
        crate::room_turns::support::load_participant(tx, &room_id, &participant_id).await?;
    let incarnation = parse_uuid(row.get("room_uid"))?;
    if room.room_uid != incarnation
        || participant.participant_type != "agent"
        || participant.room_id != room_id
        || participant.participant_id != participant_id
    {
        return Err(rejected(
            "session_revoked",
            "The connector room or participant changed.",
        ));
    }
    let invite_scope = match row.get::<&str, _>("scope") {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => {
            return Err(rejected(
                "invalid_state",
                "Stored connector scope is invalid.",
            ));
        }
    };
    let principal = AuthenticatedPrincipal {
        principal_id: participant_id.clone(),
        participant_id,
        display_name: participant.display_name,
        room_id,
        client_kind: ClientKind::RoomConnector,
        invite_scope,
        is_operator: false,
        capabilities: CapabilitySet::for_principal(ClientKind::RoomConnector, invite_scope, false),
    };
    Ok(ConnectorSessionRecord {
        principal,
        room_uid: incarnation,
        expires_at,
        revoked: row.get::<i64, _>("revoked") != 0,
        status: participant.status,
    })
}

async fn leave_receipt_in(
    tx: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
) -> Result<Option<crate::CommandOutcome>, PersistenceError> {
    crate::command_admission::inspect_non_lifecycle_command(
        tx,
        &principal.room_id,
        &principal.principal_id,
        request_id,
        "participant.leave",
        &agentsassemble_domain::canonical_payload_hash(&json!({})),
    )
    .await
}

pub(super) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

// Agent participant kind does not imply custody of a server-managed provider process.
pub(crate) async fn owns_participant(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<bool, PersistenceError> {
    Ok(sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM room_connector_invites WHERE room_id=? AND participant_id=?)",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_one(&mut **tx)
    .await?)
}
