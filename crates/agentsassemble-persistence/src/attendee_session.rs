use crate::{
    PersistenceError, SqliteStore,
    attendee_invites::{parse_uuid, rejected, timestamp},
};
use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

/// Persistence-issued attendee provenance; never interchangeable with a human or managed runtime.
#[derive(Clone)]
pub struct AttendeeSessionAuthorization {
    pub(crate) fingerprint: [u8; 32],
    pub(crate) room_uid: Uuid,
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) provider_kind: String,
    pub(crate) expires_at: DateTime<Utc>,
}

impl AttendeeSessionAuthorization {
    #[must_use]
    pub const fn principal(&self) -> &AuthenticatedPrincipal {
        &self.principal
    }
    #[must_use]
    pub const fn room_uid(&self) -> Uuid {
        self.room_uid
    }
    #[must_use]
    pub const fn session_fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    #[must_use]
    pub fn provider_kind(&self) -> &str {
        &self.provider_kind
    }
    #[must_use]
    pub const fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }
}

impl SqliteStore {
    /// Authorizes only a currently admitted external attendee.
    ///
    /// # Errors
    /// Rejects missing, expired, revoked, replaced-room, changed-membership and ended parent authority.
    pub async fn authorize_attendee_session(
        &self,
        fingerprint: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<AttendeeSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = authorize_in(&mut tx, fingerprint, now).await?;
        tx.commit().await?;
        Ok(current)
    }

    /// Revalidates exact immutable custody before using an existing session.
    ///
    /// # Errors
    /// Rejects changed attendee provenance and propagates storage failures.
    pub async fn revalidate_attendee_session(
        &self,
        expected: &AttendeeSessionAuthorization,
        now: DateTime<Utc>,
    ) -> Result<AttendeeSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = revalidate_in(&mut tx, expected, now).await?;
        tx.commit().await?;
        Ok(current)
    }
}

pub(crate) async fn revalidate_in(
    tx: &mut Transaction<'_, Sqlite>,
    expected: &AttendeeSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<AttendeeSessionAuthorization, PersistenceError> {
    let current = authorize_in(tx, &expected.fingerprint, now).await?;
    if current.room_uid != expected.room_uid
        || current.principal.room_id != expected.principal.room_id
        || current.principal.participant_id != expected.principal.participant_id
        || current.provider_kind != expected.provider_kind
        || current.expires_at != expected.expires_at
    {
        return Err(rejected(
            "session_revoked",
            "The attendee's exact authority changed.",
        ));
    }
    Ok(current)
}

pub(crate) async fn authorize_in(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<AttendeeSessionAuthorization, PersistenceError> {
    let row = sqlx::query("SELECT * FROM room_attendee_invites WHERE session_fingerprint=?")
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| rejected("session_revoked", "The attendee session is unavailable."))?;
    let expires_at = timestamp(row.get("session_expires_at"))?;
    if row.get::<i64, _>("revoked") != 0 || expires_at <= now {
        return Err(rejected(
            "session_revoked",
            "The attendee session has ended.",
        ));
    }
    let room_id: String = row.get("room_id");
    let participant_id: String = row.get("participant_id");
    let (room, participant) =
        crate::authority::load_active_membership(tx, &room_id, &participant_id).await?;
    let incarnation = parse_uuid(row.get("room_uid"))?;
    let session = crate::agent_lifecycle::load_session(tx, &room_id, &participant_id).await?;
    if room.room_uid != incarnation
        || participant.participant_type != "agent"
        || session.public.participant_id != participant_id
        || !session.public.external_owned
        || session.public.process_ownership != "external"
        || session.public.provider_kind != row.get::<String, _>("provider_kind")
    {
        return Err(rejected(
            "session_revoked",
            "The attendee's room or provider custody changed.",
        ));
    }
    let parent: Option<Vec<u8>> = row.get("parent_fingerprint");
    if let Some(parent) = parent {
        require_parent(tx, &room_id, &parent, now).await?;
    }
    let scope = match row.get::<&str, _>("scope") {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => {
            return Err(rejected(
                "invalid_state",
                "Stored attendee scope is invalid.",
            ));
        }
    };
    let principal = AuthenticatedPrincipal {
        principal_id: participant_id.clone(),
        participant_id,
        display_name: participant.display_name,
        room_id,
        client_kind: ClientKind::AgentBridge,
        invite_scope: scope,
        is_operator: false,
        capabilities: CapabilitySet::for_principal(ClientKind::AgentBridge, scope, false),
    };
    Ok(AttendeeSessionAuthorization {
        fingerprint: *fingerprint,
        room_uid: incarnation,
        principal,
        provider_kind: session.public.provider_kind,
        expires_at,
    })
}

pub(crate) async fn require_parent(
    tx: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    fingerprint: &[u8],
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, PersistenceError> {
    let fingerprint: [u8; 32] = fingerprint
        .try_into()
        .map_err(|_| rejected("invalid_state", "Stored attendee parent is invalid."))?;
    // Select exactly one stored provenance before validating it. An invalid human
    // parent cannot fall through to operator authority (or vice versa).
    let owners: Vec<String> = sqlx::query_scalar(
        "SELECT 'human' FROM human_room_sessions WHERE session_fingerprint=? UNION ALL SELECT 'operator' FROM operator_pairings WHERE session_fingerprint=?",
    ).bind(fingerprint.as_slice()).bind(fingerprint.as_slice()).fetch_all(&mut **tx).await?;
    let (principal, expires_at) = match owners.as_slice() {
        [kind] if kind == "human" => {
            let crate::human_session_authority::ResolvedHumanSession::Live {
                authorization, ..
            } = crate::human_session_authority::resolve_human_session(
                tx,
                &fingerprint,
                Some(room_id),
                now,
            )
            .await?
            else {
                return Err(rejected(
                    "session_revoked",
                    "The companion's parent session has ended.",
                ));
            };
            (
                authorization.principal().clone(),
                authorization.expires_at(),
            )
        }
        [kind] if kind == "operator" => {
            crate::operator_pairing::require_attendee_parent(tx, &fingerprint, room_id, now).await?
        }
        _ => {
            return Err(rejected(
                "session_revoked",
                "The companion's parent authority is missing or ambiguous.",
            ));
        }
    };
    let (_, participant) =
        crate::authority::load_active_membership(tx, room_id, &principal.participant_id).await?;
    if principal.invite_scope != InviteScope::ReadWrite || participant.muted {
        return Err(rejected(
            "permission_denied",
            "The companion's posting authority is unavailable.",
        ));
    }
    Ok(expires_at)
}
