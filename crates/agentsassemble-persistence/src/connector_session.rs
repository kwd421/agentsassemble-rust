use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, ParticipantStatus,
};
use chrono::{DateTime, Utc};
use serde_json::json;
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
        let current = revalidate_in(&mut tx, expected, Utc::now()).await?;
        let principal = current.principal();
        let payload = json!({});
        let payload_hash = agentsassemble_domain::canonical_payload_hash(&payload);
        if crate::command_admission::inspect_non_lifecycle_command(
            &mut tx,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            "participant.leave",
            &payload_hash,
        )
        .await?
        .is_some()
        {
            return Err(PersistenceError::CommandConflict);
        }
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
    let current = authorize_in(tx, &expected.fingerprint, now).await?;
    if current.room_uid != expected.room_uid
        || current.principal.room_id != expected.principal.room_id
        || current.principal.participant_id != expected.principal.participant_id
        || current.expires_at != expected.expires_at
    {
        return Err(rejected(
            "session_revoked",
            "The connector's exact room authority changed.",
        ));
    }
    Ok(current)
}

pub(super) async fn authorize_in(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<ConnectorSessionAuthorization, PersistenceError> {
    let row=sqlx::query("SELECT room_id,room_uid,participant_id,scope,revoked,session_expires_at FROM room_connector_invites WHERE session_fingerprint=?")
        .bind(fingerprint.as_slice()).fetch_optional(&mut **tx).await?.ok_or_else(||rejected("session_revoked","The connector session is unavailable."))?;
    let expires_at = timestamp(row.get("session_expires_at"))?;
    if row.get::<i64, _>("revoked") != 0 || expires_at <= now {
        return Err(rejected(
            "session_revoked",
            "The connector session has ended.",
        ));
    }
    let room_id: String = row.get("room_id");
    let participant_id: String = row.get("participant_id");
    let (room, participant) =
        crate::authority::load_active_membership(tx, &room_id, &participant_id).await?;
    let incarnation = parse_uuid(row.get("room_uid"))?;
    if room.room_uid != incarnation || participant.participant_type != "agent" {
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
    Ok(ConnectorSessionAuthorization {
        principal,
        fingerprint: *fingerprint,
        room_uid: incarnation,
        expires_at,
    })
}

pub(super) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
