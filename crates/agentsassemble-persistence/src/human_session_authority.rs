use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant, ParticipantStatus,
    Room, RoomStatus, UserProfile,
};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore, profile_store::decode_bound_profile};

/// Current durable authority behind one presented human room-session fingerprint.
///
/// The raw bearer is intentionally absent. Private fields prevent another crate from
/// constructing claimed session provenance without the persistence owner.
pub struct HumanSessionAuthorization {
    session_fingerprint: [u8; 32],
    principal: AuthenticatedPrincipal,
    expires_at: DateTime<Utc>,
}

pub(crate) enum ResolvedHumanSession {
    Missing,
    ForeignRoom,
    Unavailable,
    Live {
        authorization: HumanSessionAuthorization,
        profile: Box<UserProfile>,
    },
}

impl HumanSessionAuthorization {
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
    /// Resolves a live human session and all authority it depends on in one snapshot.
    ///
    /// # Errors
    ///
    /// Fails when the session is missing, ended, expired, or no longer owns an active
    /// room, joined human participant, and exact profile binding. Corrupt durable
    /// records fail as invalid state rather than becoming unavailable authority.
    pub async fn authorize_human_session(
        &self,
        session_fingerprint: &[u8; 32],
    ) -> Result<HumanSessionAuthorization, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let resolution =
            resolve_human_session(&mut transaction, session_fingerprint, None, Utc::now()).await?;
        transaction.commit().await?;
        match resolution {
            ResolvedHumanSession::Live { authorization, .. } => Ok(authorization),
            ResolvedHumanSession::Missing
            | ResolvedHumanSession::ForeignRoom
            | ResolvedHumanSession::Unavailable => Err(session_revoked()),
        }
    }
}

pub(crate) async fn resolve_human_session(
    transaction: &mut Transaction<'_, Sqlite>,
    session_fingerprint: &[u8; 32],
    expected_room_id: Option<&str>,
    now: DateTime<Utc>,
) -> Result<ResolvedHumanSession, PersistenceError> {
    let row = sqlx::query(
            "SELECT sessions.room_id, sessions.user_id, sessions.participant_id, sessions.client_kind, sessions.invite_scope, sessions.expires_at, sessions.state, profiles.participant_id AS profile_participant_id, profiles.profile_json, participants.participant_json, rooms.room_json FROM human_room_sessions AS sessions LEFT JOIN user_profiles AS profiles ON profiles.user_id = sessions.user_id LEFT JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = sessions.participant_id LEFT JOIN rooms ON rooms.room_id = sessions.room_id WHERE sessions.session_fingerprint = ?",
        )
        .bind(session_fingerprint.as_slice())
        .fetch_optional(&mut **transaction)
        .await?;
    let Some(row) = row else {
        return Ok(ResolvedHumanSession::Missing);
    };
    let room_id = row.try_get::<String, _>("room_id")?;
    if expected_room_id.is_some_and(|expected| expected != room_id) {
        return Ok(ResolvedHumanSession::ForeignRoom);
    }

    let state = row.try_get::<String, _>("state")?;
    if !matches!(state.as_str(), "active" | "ended") {
        return Err(invalid_state("Stored human session state is invalid."));
    }
    let expires_at = DateTime::from_timestamp_micros(row.try_get("expires_at")?)
        .ok_or_else(|| invalid_state("Stored human session expiry is invalid."))?;
    if state != "active" || expires_at <= now {
        return Ok(ResolvedHumanSession::Unavailable);
    }
    if row.try_get::<String, _>("client_kind")? != "browser" {
        return Err(invalid_state("Stored human session client is invalid."));
    }
    let invite_scope = match row.try_get::<String, _>("invite_scope")?.as_str() {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => return Err(invalid_state("Stored human session scope is invalid.")),
    };
    let user_id = row.try_get::<String, _>("user_id")?;
    let participant_id = row.try_get::<String, _>("participant_id")?;
    let room: Room = decode_required_json(&row, "room_json", "room")?;
    let participant: Participant = decode_required_json(&row, "participant_json", "participant")?;
    let profile = decode_bound_profile(
        required_text(&row, "profile_participant_id", "profile")?,
        &participant_id,
        required_text(&row, "profile_json", "profile")?,
    )?;
    if room.room_id != room_id {
        return Err(invalid_state(
            "Stored human session room binding is invalid.",
        ));
    }
    if room.status != RoomStatus::Active {
        return Ok(ResolvedHumanSession::Unavailable);
    }
    if participant.room_id != room_id
        || participant.participant_id != participant_id
        || participant.participant_type != "human"
    {
        return Err(invalid_state(
            "Stored human session participant binding is invalid.",
        ));
    }
    if participant.status != ParticipantStatus::Joined {
        return Ok(ResolvedHumanSession::Unavailable);
    }
    if user_id.is_empty() || participant_id.is_empty() {
        return Err(invalid_state("Stored human session identity is invalid."));
    }
    let client_kind = ClientKind::Browser;
    Ok(ResolvedHumanSession::Live {
        authorization: HumanSessionAuthorization {
            session_fingerprint: *session_fingerprint,
            principal: AuthenticatedPrincipal {
                principal_id: user_id,
                participant_id,
                display_name: profile.display_name.clone(),
                room_id,
                client_kind,
                invite_scope,
                is_operator: false,
                capabilities: CapabilitySet::for_principal(client_kind, invite_scope, false),
            },
            expires_at,
        },
        profile: Box::new(profile),
    })
}

pub(crate) async fn revalidate_human_session(
    transaction: &mut Transaction<'_, Sqlite>,
    expected: &HumanSessionAuthorization,
    now: DateTime<Utc>,
) -> Result<(HumanSessionAuthorization, UserProfile), PersistenceError> {
    let resolution =
        resolve_human_session(transaction, expected.session_fingerprint(), None, now).await?;
    let ResolvedHumanSession::Live {
        authorization,
        profile,
    } = resolution
    else {
        return Err(session_revoked());
    };
    if !same_provenance(&authorization, expected) {
        return Err(invalid_state(
            "Stored human session provenance changed after grant issuance.",
        ));
    }
    Ok((authorization, *profile))
}

fn same_provenance(
    current: &HumanSessionAuthorization,
    expected: &HumanSessionAuthorization,
) -> bool {
    let current_principal = current.principal();
    let expected_principal = expected.principal();
    current.session_fingerprint == expected.session_fingerprint
        && current.expires_at == expected.expires_at
        && current_principal.principal_id == expected_principal.principal_id
        && current_principal.participant_id == expected_principal.participant_id
        && current_principal.room_id == expected_principal.room_id
        && current_principal.client_kind == expected_principal.client_kind
        && current_principal.invite_scope == expected_principal.invite_scope
        && current_principal.is_operator == expected_principal.is_operator
        && current_principal.capabilities == expected_principal.capabilities
}

fn required_text<'a>(
    row: &'a sqlx::sqlite::SqliteRow,
    column: &str,
    owner: &str,
) -> Result<&'a str, PersistenceError> {
    row.try_get::<Option<&str>, _>(column)?
        .ok_or_else(|| invalid_state(format!("Stored human session {owner} is missing.")))
}

fn decode_required_json<T: serde::de::DeserializeOwned>(
    row: &sqlx::sqlite::SqliteRow,
    column: &str,
    owner: &str,
) -> Result<T, PersistenceError> {
    serde_json::from_str(required_text(row, column, owner)?).map_err(PersistenceError::from)
}

fn session_revoked() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "session_revoked",
        message: "This human room session has ended.".to_owned(),
    }
}

fn invalid_state(message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: message.into(),
    }
}
