use agentsassemble_domain::{
    FriendParticipantType, InviteScope, SavedFriend, canonical_payload_hash,
};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    HumanSessionAuthorization, PersistenceError, RoomManagerAuthority, RoomUserIdentity,
    SqliteStore,
    human_session_authority::revalidate_human_session,
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

/// Private invitation custody; never included in room events or provider input.
pub struct AttendeeInvite {
    pub invite_id: Uuid,
    pub invite_bearer: String,
    pub room_uid: Uuid,
    pub provider_kind: String,
    pub display_name: String,
    pub expires_at: DateTime<Utc>,
}

pub struct CompanionInviteRequest<'a> {
    pub request_id: Uuid,
    pub provider_kind: &'a str,
    pub display_name: &'a str,
}

struct InvitationOwner {
    identity: RoomUserIdentity,
    parent: Option<[u8; 32]>,
    expires_at: DateTime<Utc>,
}

impl SqliteStore {
    /// Issues an external attendee invitation from current saved contact metadata.
    ///
    /// # Errors
    /// Rejects changed manager authority, conflicting retries, missing/non-AI contacts or storage failure.
    pub async fn create_friend_attendee_invite(
        &self,
        manager: &RoomManagerAuthority,
        request_id: Uuid,
        friend_id: Uuid,
        resolve_provider: impl FnOnce(&str) -> Option<&'static str>,
        now: DateTime<Utc>,
    ) -> Result<AttendeeInvite, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let identity = manager.resolve(&mut tx).await?;
        let owner = InvitationOwner {
            identity,
            parent: None,
            expires_at: now + Duration::hours(1),
        };
        let hash = canonical_payload_hash(&json!({"friend_id":friend_id}));
        if let Some(invite) = self
            .replay_attendee_invite(&mut tx, &owner, request_id, &hash, now)
            .await?
        {
            tx.commit().await?;
            return Ok(invite);
        }
        let raw: Option<String> = sqlx::query_scalar(
            "SELECT friend_json FROM saved_friends WHERE friend_id=? AND friend_json IS NOT NULL",
        )
        .bind(friend_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let friend: SavedFriend = serde_json::from_str(&raw.ok_or_else(|| {
            rejected("friend_unavailable", "The saved AI contact is unavailable.")
        })?)?;
        if matches!(
            friend.details.participant_type,
            FriendParticipantType::Human | FriendParticipantType::Unknown
        ) {
            return Err(rejected(
                "friend_not_ai",
                "An AI contact is required for attendee admission.",
            ));
        }
        // Registration policy belongs to the provider crate. Resolve the exact stored contact
        // inside this transaction; no stale preflight read or second provider registry is needed.
        let provider_kind = resolve_provider(&friend.details.provider_kind).ok_or_else(|| {
            rejected(
                "unsupported_provider",
                "The saved contact does not select a supported attendee provider.",
            )
        })?;
        let invite = self
            .insert_attendee_invite(
                &mut tx,
                &owner,
                &CompanionInviteRequest {
                    request_id,
                    provider_kind,
                    display_name: &friend.details.display_name,
                },
                &hash,
            )
            .await?;
        tx.commit().await?;
        Ok(invite)
    }

    /// Creates an attendee packet under a current posting human's room custody.
    ///
    /// # Errors
    /// Rejects revoked/read-only/muted owners, conflicting retries and the existing eight-companion limit.
    pub async fn create_companion_attendee_invite(
        &self,
        human: &HumanSessionAuthorization,
        request: CompanionInviteRequest<'_>,
        now: DateTime<Utc>,
    ) -> Result<AttendeeInvite, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let (current, _) = revalidate_human_session(&mut tx, human, now).await?;
        let principal = current.principal();
        let (_, participant) = crate::authority::load_active_membership(
            &mut tx,
            &principal.room_id,
            &principal.participant_id,
        )
        .await?;
        if principal.invite_scope != InviteScope::ReadWrite || participant.muted {
            return Err(rejected(
                "permission_denied",
                "A current posting human session is required.",
            ));
        }
        let owner = InvitationOwner {
            identity: RoomUserIdentity {
                room_id: principal.room_id.clone(),
                user_id: principal.principal_id.clone(),
                participant_id: principal.participant_id.clone(),
            },
            parent: Some(*current.session_fingerprint()),
            expires_at: (now + Duration::minutes(10)).min(current.expires_at()),
        };
        let hash = canonical_payload_hash(
            &json!({"provider_kind":request.provider_kind,"display_name":request.display_name}),
        );
        if let Some(invite) = self
            .replay_attendee_invite(&mut tx, &owner, request.request_id, &hash, now)
            .await?
        {
            tx.commit().await?;
            return Ok(invite);
        }
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM room_attendee_invites WHERE room_id=? AND creator_id=? AND parent_fingerprint IS NOT NULL AND revoked=0 AND ((session_fingerprint IS NULL AND expires_at>?) OR session_expires_at>?)")
            .bind(&owner.identity.room_id).bind(&owner.identity.user_id).bind(now.timestamp_micros()).bind(now.timestamp_micros()).fetch_one(&mut *tx).await?;
        if count >= 8 {
            return Err(rejected(
                "companion_limit_reached",
                "This room already has eight pending or admitted companions for this owner.",
            ));
        }
        let invite = self
            .insert_attendee_invite(&mut tx, &owner, &request, &hash)
            .await?;
        tx.commit().await?;
        Ok(invite)
    }

    async fn replay_attendee_invite(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        owner: &InvitationOwner,
        request_id: Uuid,
        hash: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<AttendeeInvite>, PersistenceError> {
        if request_id.is_nil() {
            return Err(rejected(
                "bad_request",
                "A nonzero request UUID is required.",
            ));
        }
        let identity = &owner.identity;
        let room = crate::authority::load_active_room(tx, &identity.room_id).await?;
        let row = sqlx::query(
            "SELECT * FROM room_attendee_invites WHERE room_id=? AND creator_id=? AND request_id=?",
        )
        .bind(&identity.room_id)
        .bind(&identity.user_id)
        .bind(request_id.to_string())
        .fetch_optional(&mut **tx)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.get::<String, _>("request_hash") != hash
            || row
                .get::<Option<Vec<u8>>, _>("parent_fingerprint")
                .as_deref()
                != owner.parent.as_ref().map(<[u8; 32]>::as_slice)
        {
            return Err(PersistenceError::CommandConflict);
        }
        let expires_at = timestamp(row.get("expires_at"))?;
        if row.get::<i64, _>("revoked") != 0
            || expires_at <= now
            || row.get::<String, _>("room_uid") != room.room_uid.to_string()
        {
            return Err(rejected(
                "invite_unavailable",
                "This attendee invitation has ended.",
            ));
        }
        let bearer = self.attendee_invite_bearer(room.room_uid, &identity.user_id, request_id);
        Ok(Some(AttendeeInvite {
            invite_id: parse_uuid(row.get("invite_id"))?,
            invite_bearer: bearer.bearer,
            room_uid: room.room_uid,
            provider_kind: row.get("provider_kind"),
            display_name: row.get("display_name"),
            expires_at,
        }))
    }

    async fn insert_attendee_invite(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        owner: &InvitationOwner,
        request: &CompanionInviteRequest<'_>,
        hash: &str,
    ) -> Result<AttendeeInvite, PersistenceError> {
        if !valid_text(request.provider_kind, 64) || !valid_text(request.display_name, 120) {
            return Err(rejected(
                "bad_request",
                "An explicit provider and bounded display name are required.",
            ));
        }
        let identity = &owner.identity;
        let room = crate::authority::load_active_room(tx, &identity.room_id).await?;
        let bearer =
            self.attendee_invite_bearer(room.room_uid, &identity.user_id, request.request_id);
        let invite_id = Uuid::new_v4();
        sqlx::query("INSERT INTO room_attendee_invites(invite_id,room_id,room_uid,creator_id,owner_participant_id,request_id,request_hash,provider_kind,display_name,scope,parent_fingerprint,token_fingerprint,expires_at) VALUES(?,?,?,?,?,?,?,?,?,'read_write',?,?,?)")
            .bind(invite_id.to_string()).bind(&identity.room_id).bind(room.room_uid.to_string()).bind(&identity.user_id).bind(&identity.participant_id)
            .bind(request.request_id.to_string()).bind(hash).bind(request.provider_kind).bind(request.display_name)
            .bind(owner.parent.as_ref().map(<[u8;32]>::as_slice)).bind(bearer.fingerprint.as_slice()).bind(owner.expires_at.timestamp_micros()).execute(&mut **tx).await?;
        Ok(AttendeeInvite {
            invite_id,
            invite_bearer: bearer.bearer,
            room_uid: room.room_uid,
            provider_kind: request.provider_kind.to_owned(),
            display_name: request.display_name.to_owned(),
            expires_at: owner.expires_at,
        })
    }

    fn attendee_invite_bearer(
        &self,
        room_uid: Uuid,
        creator_id: &str,
        request_id: Uuid,
    ) -> crate::session_bearer::IssuedBearer {
        let mut seed = Sha256::new();
        seed.update(room_uid.as_bytes());
        seed.update(request_id.as_bytes());
        seed.update(creator_id.as_bytes());
        derive_session_bearer(
            self.host_key.session_hmac_key(),
            &seed.finalize().into(),
            SessionBearerPurpose::AttendeeInvite,
        )
    }
}

pub(super) fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}

pub(super) fn timestamp(micros: i64) -> Result<DateTime<Utc>, PersistenceError> {
    DateTime::from_timestamp_micros(micros)
        .ok_or_else(|| rejected("invalid_state", "Stored attendee expiry is invalid."))
}

pub(super) fn parse_uuid(value: &str) -> Result<Uuid, PersistenceError> {
    Uuid::parse_str(value)
        .map_err(|_| rejected("invalid_state", "Stored attendee identity is invalid."))
}

fn valid_text(value: &str, limit: usize) -> bool {
    !value.trim().is_empty()
        && value.chars().count() <= limit
        && !value.chars().any(char::is_control)
}
