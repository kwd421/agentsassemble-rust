use crate::{
    AttendeeSessionAuthorization, PersistenceError, SqliteStore,
    attendee_invites::{parse_uuid, rejected},
    attendee_session::{authorize_in, require_parent},
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};
use chrono::{DateTime, Duration, Utc};
use sqlx::Row;
use uuid::Uuid;

/// Client-owned admission identity. The provider is explicit and must match the invitation.
pub struct AttendeeAdmissionRequest<'a> {
    pub invite_fingerprint: &'a [u8; 32],
    pub client_fingerprint: &'a [u8; 32],
    pub request_id: Uuid,
    pub provider_kind: &'a str,
    pub display_name: &'a str,
}

/// Only the admission transport receives the credential. Public events contain no bearer.
pub struct AttendeeAdmission {
    pub session_bearer: String,
    pub authorization: AttendeeSessionAuthorization,
    pub event: agentsassemble_domain::RoomEvent,
    pub deduplicated: bool,
}

impl SqliteStore {
    /// Resolves the owning room queue without granting admission authority.
    ///
    /// # Errors
    /// Propagates database failure rather than treating it as an unknown invitation.
    pub async fn attendee_admission_room_id(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<Option<String>, PersistenceError> {
        Ok(sqlx::query_scalar(
            "SELECT room_id FROM room_attendee_invites WHERE token_fingerprint=?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Consumes one exact attendee invitation with its membership and Agent Session event.
    ///
    /// # Errors
    /// Rejects wrong providers, revised or competing admissions, expiry and stale parent/room custody.
    pub async fn admit_attendee(
        &self,
        request: AttendeeAdmissionRequest<'_>,
        now: DateTime<Utc>,
    ) -> Result<AttendeeAdmission, PersistenceError> {
        if request.request_id.is_nil()
            || request.display_name.trim().is_empty()
            || request.display_name.chars().count() > 120
            || request.display_name.chars().any(char::is_control)
        {
            return Err(rejected(
                "bad_request",
                "A request UUID and bounded attendee display name are required.",
            ));
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let row = sqlx::query("SELECT * FROM room_attendee_invites WHERE token_fingerprint=?")
            .bind(request.invite_fingerprint.as_slice())
            .fetch_optional(&mut *tx)
            .await?
            .ok_or_else(|| {
                rejected(
                    "invite_unavailable",
                    "The attendee invitation is unavailable.",
                )
            })?;
        if row.get::<i64, _>("revoked") != 0 {
            return Err(rejected(
                "invite_unavailable",
                "The attendee invitation has ended.",
            ));
        }
        if row.get::<String, _>("provider_kind") != request.provider_kind {
            return Err(rejected(
                "provider_mismatch",
                "The explicit provider does not match this attendee invitation.",
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
        let mut expires_at = now + Duration::hours(1);
        if let Some(parent) = row.get::<Option<Vec<u8>>, _>("parent_fingerprint") {
            expires_at = expires_at.min(require_parent(&mut tx, &room_id, &parent, now).await?);
        }
        let bearer = derive_session_bearer(
            self.host_key.session_hmac_key(),
            request.invite_fingerprint,
            SessionBearerPurpose::AttendeeSession,
        );
        if let Some(client) = row.get::<Option<Vec<u8>>, _>("client_fingerprint") {
            if client.as_slice() != request.client_fingerprint
                || row.get::<String, _>("join_request_id") != request.request_id.to_string()
            {
                return Err(rejected(
                    "invite_already_used",
                    "The invitation was consumed by another admission.",
                ));
            }
            let (authorization, event) =
                replay_admission(&mut tx, &room_id, &row, &request, &bearer.fingerprint, now)
                    .await?;
            tx.commit().await?;
            return Ok(AttendeeAdmission {
                session_bearer: bearer.bearer,
                authorization,
                event,
                deduplicated: true,
            });
        }
        if row.get::<i64, _>("expires_at") <= now.timestamp_micros() {
            return Err(rejected(
                "invite_unavailable",
                "The attendee invitation expired.",
            ));
        }
        let event = crate::attendee_records::insert_membership(
            &mut tx,
            &room_id,
            row.get("owner_participant_id"),
            request.provider_kind,
            request.display_name,
            now,
        )
        .await?;
        sqlx::query("UPDATE room_attendee_invites SET client_fingerprint=?,join_request_id=?,participant_id=?,event_id=?,session_fingerprint=?,session_expires_at=? WHERE invite_id=?")
            .bind(request.client_fingerprint.as_slice()).bind(request.request_id.to_string()).bind(&event.actor.participant_id).bind(&event.id)
            .bind(bearer.fingerprint.as_slice()).bind(expires_at.timestamp_micros()).bind(row.get::<String,_>("invite_id")).execute(&mut *tx).await?;
        let authorization = authorize_in(&mut tx, &bearer.fingerprint, now).await?;
        tx.commit().await?;
        Ok(AttendeeAdmission {
            session_bearer: bearer.bearer,
            authorization,
            event,
            deduplicated: false,
        })
    }
}

async fn replay_admission(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
    row: &sqlx::sqlite::SqliteRow,
    request: &AttendeeAdmissionRequest<'_>,
    session_fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<
    (
        AttendeeSessionAuthorization,
        agentsassemble_domain::RoomEvent,
    ),
    PersistenceError,
> {
    let authorization = authorize_in(tx, session_fingerprint, now).await?;
    let event =
        crate::room_turns::support::load_event(tx, room_id, &row.get::<String, _>("event_id"))
            .await?
            .ok_or_else(|| {
                rejected(
                    "invalid_state",
                    "The committed attendee admission event is missing.",
                )
            })?;
    if event.display_name.as_deref() != Some(request.display_name) {
        return Err(rejected(
            "invite_already_used",
            "The committed attendee admission cannot change its original payload.",
        ));
    }
    Ok((authorization, event))
}
