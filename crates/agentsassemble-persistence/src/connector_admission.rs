use agentsassemble_domain::{
    Actor, InviteScope, Participant, ParticipantRole, ParticipantStatus, RoomEvent,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    PersistenceError, RoomManagerAuthority, SqliteStore,
    connector_session::{ConnectorSessionAuthorization, authorize_in, rejected},
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

const TTL: Duration = Duration::hours(1);

/// Secret-bearing result, intentionally neither Debug nor Serialize.
pub struct ConnectorInvite {
    pub invite_id: Uuid,
    pub invite_bearer: String,
    pub expires_at: DateTime<Utc>,
}

/// Only the transport receives the credential; the room event is public and raw-free.
pub struct ConnectorAdmission {
    pub session_bearer: String,
    pub authorization: ConnectorSessionAuthorization,
    pub event: RoomEvent,
    pub deduplicated: bool,
}

impl SqliteStore {
    /// Creates or recovers one exact current-session AI invitation.
    ///
    /// # Errors
    /// Rejects changed manager authority, conflicting retries or storage failure.
    pub async fn create_connector_invite(
        &self,
        manager: &RoomManagerAuthority,
        request_id: Uuid,
        scope: InviteScope,
        now: DateTime<Utc>,
    ) -> Result<ConnectorInvite, PersistenceError> {
        if request_id.is_nil() {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let identity = manager.resolve(&mut tx).await?;
        let room = crate::authority::load_active_room(&mut tx, &identity.room_id).await?;
        let scope = match scope {
            InviteScope::ReadOnly => "read_only",
            InviteScope::ReadWrite => "read_write",
        };
        let mut seed = Sha256::new();
        seed.update(room.room_uid.as_bytes());
        seed.update(request_id.as_bytes());
        let bearer = derive_session_bearer(
            self.host_key.session_hmac_key(),
            &seed.finalize().into(),
            SessionBearerPurpose::ConnectorInvite,
        );
        if let Some(row) = sqlx::query("SELECT invite_id, creator_id, scope, expires_at, revoked FROM room_connector_invites WHERE room_id = ? AND request_id = ?")
            .bind(&identity.room_id).bind(request_id.to_string()).fetch_optional(&mut *tx).await? {
            if row.get::<String,_>("creator_id") != identity.user_id || row.get::<String,_>("scope") != scope {
                return Err(PersistenceError::CommandConflict);
            }
            let expires_at = timestamp(row.get("expires_at"))?;
            if row.get::<i64,_>("revoked") != 0 || expires_at <= now { return Err(rejected("invite_unavailable", "This connector invitation has ended.")); }
            let result = ConnectorInvite { invite_id: parse_uuid(row.get("invite_id"))?, invite_bearer: bearer.bearer, expires_at };
            tx.commit().await?;
            return Ok(result);
        }
        // Retain manager-created receipts until room deletion. Removing expired
        // requests could recreate the same deterministic credential with a new expiry.
        let invite_id = Uuid::new_v4();
        let expires_at = now + TTL;
        sqlx::query("INSERT INTO room_connector_invites(invite_id,room_id,room_uid,creator_id,request_id,scope,token_fingerprint,expires_at) VALUES(?,?,?,?,?,?,?,?)")
            .bind(invite_id.to_string()).bind(&identity.room_id).bind(room.room_uid.to_string()).bind(&identity.user_id)
            .bind(request_id.to_string()).bind(scope).bind(bearer.fingerprint.as_slice()).bind(expires_at.timestamp_micros()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(ConnectorInvite {
            invite_id,
            invite_bearer: bearer.bearer,
            expires_at,
        })
    }

    /// Resolves only the room queue; admission revalidates all authority in its transaction.
    ///
    /// # Errors
    /// Propagates database failures without treating them as a missing invite.
    pub async fn connector_admission_room_id(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<Option<String>, PersistenceError> {
        Ok(sqlx::query_scalar(
            "SELECT room_id FROM room_connector_invites WHERE token_fingerprint = ?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Atomically consumes a connector invite and commits its agent participant and event.
    ///
    /// # Errors
    /// Rejects different-client/revised retries, expiry, revocation or replacement rooms.
    pub async fn admit_connector(
        &self,
        invite_fingerprint: &[u8; 32],
        client_fingerprint: &[u8; 32],
        request_id: Uuid,
        display_name: &str,
        now: DateTime<Utc>,
    ) -> Result<ConnectorAdmission, PersistenceError> {
        if request_id.is_nil()
            || display_name.trim().is_empty()
            || display_name.chars().count() > 120
            || display_name.chars().any(char::is_control)
        {
            return Err(rejected(
                "bad_request",
                "A request UUID and bounded connector display name are required.",
            ));
        }
        let payload_hash =
            agentsassemble_domain::canonical_payload_hash(&json!({"display_name":display_name}));
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM room_connector_invites WHERE token_fingerprint = ?")
            .bind(invite_fingerprint.as_slice())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                rejected(
                    "invite_unavailable",
                    "The connector invitation is unavailable.",
                )
            })?;
        if row.get::<i64, _>("revoked") != 0 {
            return Err(rejected(
                "invite_unavailable",
                "The connector invitation has ended.",
            ));
        }
        let room_id: String = row.get("room_id");
        let room = crate::authority::load_active_room(&mut tx, &room_id).await?;
        if room.room_uid != parse_uuid(row.get("room_uid"))? {
            return Err(rejected(
                "room_incarnation_changed",
                "The invited room no longer exists.",
            ));
        }
        let bearer = derive_session_bearer(
            self.host_key.session_hmac_key(),
            invite_fingerprint,
            SessionBearerPurpose::ConnectorSession,
        );
        if let Some(client) = row.get::<Option<Vec<u8>>, _>("client_fingerprint") {
            if client.as_slice() != client_fingerprint
                || row.get::<String, _>("join_request_id") != request_id.to_string()
                || row.get::<String, _>("join_payload_hash") != payload_hash
            {
                return Err(rejected(
                    "invite_already_used",
                    "This invitation was already consumed by another admission.",
                ));
            }
            let authorization = authorize_in(&mut tx, &bearer.fingerprint, now).await?;
            let event = crate::room_turns::support::load_event(
                &mut tx,
                &room_id,
                &row.get::<String, _>("event_id"),
            )
            .await?
            .ok_or_else(|| {
                rejected(
                    "invalid_state",
                    "The committed connector admission event is missing.",
                )
            })?;
            tx.commit().await?;
            return Ok(ConnectorAdmission {
                session_bearer: bearer.bearer,
                authorization,
                event,
                deduplicated: true,
            });
        }
        if row.get::<i64, _>("expires_at") <= now.timestamp_micros() {
            return Err(rejected(
                "invite_unavailable",
                "The connector invitation expired.",
            ));
        }
        let (participant, event) = insert_connector_participant(
            &mut tx,
            &room_id,
            display_name,
            row.get("creator_id"),
            now,
        )
        .await?;
        sqlx::query("UPDATE room_connector_invites SET client_fingerprint=?,join_request_id=?,join_payload_hash=?,participant_id=?,event_id=?,session_fingerprint=?,session_expires_at=? WHERE invite_id=?")
            .bind(client_fingerprint.as_slice()).bind(request_id.to_string()).bind(payload_hash).bind(&participant.participant_id).bind(&event.id)
            .bind(bearer.fingerprint.as_slice()).bind((now+TTL).timestamp_micros()).bind(row.get::<String,_>("invite_id")).execute(&mut *tx).await?;
        let authorization = authorize_in(&mut tx, &bearer.fingerprint, now).await?;
        tx.commit().await?;
        Ok(ConnectorAdmission {
            session_bearer: bearer.bearer,
            authorization,
            event,
            deduplicated: false,
        })
    }
}

async fn insert_connector_participant(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    display_name: &str,
    creator_id: &str,
    now: DateTime<Utc>,
) -> Result<(Participant, RoomEvent), PersistenceError> {
    let participant = Participant {
        room_id: room_id.to_owned(),
        participant_id: format!("connector-{}", Uuid::new_v4().simple()),
        display_name: display_name.to_owned(),
        avatar_image_url: String::new(),
        participant_type: "agent".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Agent,
        owner_id: creator_id.to_owned(),
        muted: false,
        created_at: now,
        updated_at: now,
    };
    sqlx::query("INSERT INTO participants(room_id,participant_id,participant_json) VALUES(?,?,?)")
        .bind(room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut **tx)
        .await?;
    let event = RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: crate::room_event_sequence::next_sequence(tx, room_id).await?,
        created_at: now,
        room_id: room_id.to_owned(),
        event_type: "participant_joined".to_owned(),
        actor: Actor {
            participant_id: participant.participant_id.clone(),
            participant_type: "agent".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some("agent".to_owned()),
        actor_id: Some(participant.participant_id.clone()),
        actor_type: Some("agent".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: std::collections::BTreeMap::from([("participant".to_owned(), json!(participant))]),
    };
    crate::room_turns::support::insert_event(tx, &event).await?;
    Ok((participant, event))
}

pub(super) fn timestamp(micros: i64) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| rejected("invalid_state", "Stored connector expiry is invalid."))
}

pub(super) fn parse_uuid(value: &str) -> Result<Uuid, PersistenceError> {
    Uuid::parse_str(value)
        .map_err(|_| rejected("invalid_state", "Stored connector identity is invalid."))
}
