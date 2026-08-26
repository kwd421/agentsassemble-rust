use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant,
    ParticipantStatus, Room, RoomStatus, UserProfile,
};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, Transaction, sqlite::SqliteRow};

use crate::{HumanInvite, PersistenceError, SqliteStore, human_invites::decode_human_invite};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanInviteCredentialEvidence {
    Signed {
        fingerprint: [u8; 32],
        room_id: String,
        base_participant_id: String,
        display_name: String,
        invite_scope: InviteScope,
        issued_at: DateTime<Utc>,
        expires_at: DateTime<Utc>,
    },
    JoinCode {
        fingerprint: [u8; 32],
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInvitePreflightRequest {
    pub credential: HumanInviteCredentialEvidence,
    pub session_fingerprint: Option<[u8; 32]>,
    pub browser_credential_fingerprint: Option<[u8; 32]>,
    pub now: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInvitePreflightContext {
    pub room_id: String,
    pub room_label: String,
    pub invite_scope: InviteScope,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanInvitePreflightPerson {
    pub participant_id: String,
    pub display_name: String,
    pub avatar_image_url: String,
    pub operator: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HumanInvitePreflightRejection {
    InviteNotFound,
    InviteRevoked,
    InviteExpired,
    InviteUseLimitReached,
    RoomUnavailable,
    SessionUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HumanInvitePreflight {
    Rejected(HumanInvitePreflightRejection),
    ProfileRequired(HumanInvitePreflightContext),
    ExistingSession {
        context: HumanInvitePreflightContext,
        person: HumanInvitePreflightPerson,
    },
    KnownUser {
        context: HumanInvitePreflightContext,
        person: HumanInvitePreflightPerson,
    },
    ExistingMember {
        context: HumanInvitePreflightContext,
        person: HumanInvitePreflightPerson,
    },
}

impl SqliteStore {
    /// Resolves current browser invite startup state from one read-only snapshot.
    ///
    /// # Errors
    ///
    /// Fails on database errors or malformed/cross-bound durable authority.
    pub async fn preflight_human_invite(
        &self,
        request: &HumanInvitePreflightRequest,
    ) -> Result<HumanInvitePreflight, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let Some((invite, room)) =
            load_invite_and_room(&mut transaction, &request.credential).await?
        else {
            transaction.commit().await?;
            return Ok(HumanInvitePreflight::Rejected(
                HumanInvitePreflightRejection::InviteNotFound,
            ));
        };
        require_credential_binding(&invite, &request.credential)?;

        let rejection = if invite.revoked {
            Some(HumanInvitePreflightRejection::InviteRevoked)
        } else if invite.expires_at <= request.now {
            Some(HumanInvitePreflightRejection::InviteExpired)
        } else if invite.use_count >= invite.effective_use_limit() {
            Some(HumanInvitePreflightRejection::InviteUseLimitReached)
        } else if room.status != RoomStatus::Active {
            Some(HumanInvitePreflightRejection::RoomUnavailable)
        } else {
            None
        };
        if let Some(rejection) = rejection {
            transaction.commit().await?;
            return Ok(HumanInvitePreflight::Rejected(rejection));
        }

        let context = HumanInvitePreflightContext {
            room_id: invite.room_id.clone(),
            room_label: room.label,
            invite_scope: invite.invite_scope,
        };
        let session = match request.session_fingerprint {
            Some(fingerprint) => Some(
                load_presented_session(
                    &mut transaction,
                    &context.room_id,
                    &fingerprint,
                    request.now,
                )
                .await?,
            ),
            None => None,
        };
        let decision = match session {
            Some(PresentedSession::Live {
                person,
                invite_scope,
            }) => HumanInvitePreflight::ExistingSession {
                context: HumanInvitePreflightContext {
                    invite_scope,
                    ..context
                },
                person,
            },
            Some(PresentedSession::Unavailable) => {
                HumanInvitePreflight::Rejected(HumanInvitePreflightRejection::SessionUnavailable)
            }
            None => {
                if let Some(fingerprint) = request.browser_credential_fingerprint {
                    match load_device_person(&mut transaction, &context.room_id, &fingerprint)
                        .await?
                    {
                        Some((person, true)) => {
                            HumanInvitePreflight::ExistingMember { context, person }
                        }
                        Some((person, false)) => {
                            HumanInvitePreflight::KnownUser { context, person }
                        }
                        None => HumanInvitePreflight::ProfileRequired(context),
                    }
                } else {
                    HumanInvitePreflight::ProfileRequired(context)
                }
            }
        };
        transaction.commit().await?;
        Ok(decision)
    }
}

async fn load_invite_and_room(
    transaction: &mut Transaction<'_, Sqlite>,
    credential: &HumanInviteCredentialEvidence,
) -> Result<Option<(HumanInvite, Room)>, PersistenceError> {
    let row = match credential {
        HumanInviteCredentialEvidence::Signed { fingerprint, .. } => sqlx::query(
            "SELECT invites.invite_id, invites.signed_token_fingerprint, invites.join_code_fingerprint, invites.room_id, invites.base_participant_id, invites.display_name, invites.invite_scope, invites.max_uses, invites.use_count, invites.expires_at, invites.revoked, invites.created_by_user_id, invites.created_at, rooms.room_json FROM room_invites AS invites JOIN rooms ON rooms.room_id = invites.room_id WHERE invites.signed_token_fingerprint = ?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **transaction)
        .await?,
        HumanInviteCredentialEvidence::JoinCode { fingerprint } => sqlx::query(
            "SELECT invites.invite_id, invites.signed_token_fingerprint, invites.join_code_fingerprint, invites.room_id, invites.base_participant_id, invites.display_name, invites.invite_scope, invites.max_uses, invites.use_count, invites.expires_at, invites.revoked, invites.created_by_user_id, invites.created_at, rooms.room_json FROM room_invites AS invites JOIN rooms ON rooms.room_id = invites.room_id WHERE invites.join_code_fingerprint = ?",
        )
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **transaction)
        .await?,
    };
    row.as_ref().map(decode_invite_room).transpose()
}

fn decode_invite_room(row: &SqliteRow) -> Result<(HumanInvite, Room), PersistenceError> {
    let invite = decode_human_invite(row)?;
    let room: Room = serde_json::from_str(row.try_get::<String, _>("room_json")?.as_str())?;
    if room.room_id != invite.room_id || room.label.trim().is_empty() {
        return Err(invalid_state("Stored invite room authority is invalid."));
    }
    Ok((invite, room))
}

fn require_credential_binding(
    invite: &HumanInvite,
    credential: &HumanInviteCredentialEvidence,
) -> Result<(), PersistenceError> {
    if let HumanInviteCredentialEvidence::Signed {
        room_id,
        base_participant_id,
        display_name,
        invite_scope,
        issued_at,
        expires_at,
        ..
    } = credential
        && (room_id != &invite.room_id
            || base_participant_id != &invite.base_participant_id
            || display_name != &invite.display_name
            || invite_scope != &invite.invite_scope
            || issued_at != &invite.created_at
            || expires_at != &invite.expires_at)
    {
        return Err(PersistenceError::InvalidHumanInvite);
    }
    Ok(())
}

enum PresentedSession {
    Unavailable,
    Live {
        person: HumanInvitePreflightPerson,
        invite_scope: InviteScope,
    },
}

async fn load_presented_session(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) -> Result<PresentedSession, PersistenceError> {
    let row = sqlx::query(
        "SELECT sessions.room_id AS session_room_id, sessions.user_id, sessions.participant_id, sessions.client_kind, sessions.invite_scope AS session_invite_scope, sessions.state AS session_state, sessions.expires_at AS session_expires_at, profiles.profile_json, participants.participant_json FROM human_room_sessions AS sessions LEFT JOIN user_profiles AS profiles ON profiles.user_id = sessions.user_id AND profiles.participant_id = sessions.participant_id LEFT JOIN participants ON participants.room_id = sessions.room_id AND participants.participant_id = sessions.participant_id WHERE sessions.session_fingerprint = ?",
    )
    .bind(fingerprint.as_slice())
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(PresentedSession::Unavailable);
    };
    if row.try_get::<String, _>("session_room_id")? != room_id {
        return Ok(PresentedSession::Unavailable);
    }
    if row.try_get::<String, _>("client_kind")? != "browser" {
        return Err(invalid_state("Stored human session client is invalid."));
    }
    let invite_scope = match row.try_get::<String, _>("session_invite_scope")?.as_str() {
        "read_write" => InviteScope::ReadWrite,
        "read_only" => InviteScope::ReadOnly,
        _ => return Err(invalid_state("Stored human session scope is invalid.")),
    };
    let state = row.try_get::<String, _>("session_state")?;
    if !matches!(state.as_str(), "active" | "ended") {
        return Err(invalid_state("Stored human session state is invalid."));
    }
    if state != "active" || row.try_get::<i64, _>("session_expires_at")? <= now.timestamp_micros() {
        return Ok(PresentedSession::Unavailable);
    }
    let (person, joined) = decode_person(&row, room_id, Some("participant_json"))?;
    if !joined {
        return Ok(PresentedSession::Unavailable);
    }
    Ok(PresentedSession::Live {
        person,
        invite_scope,
    })
}

async fn load_device_person(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    fingerprint: &[u8; 32],
) -> Result<Option<(HumanInvitePreflightPerson, bool)>, PersistenceError> {
    let row = sqlx::query(
        "SELECT credentials.user_id, profiles.participant_id, profiles.profile_json, participants.participant_json FROM human_device_credentials AS credentials JOIN user_profiles AS profiles ON profiles.user_id = credentials.user_id LEFT JOIN participants ON participants.room_id = ? AND participants.participant_id = profiles.participant_id WHERE credentials.credential_fingerprint = ?",
    )
    .bind(room_id)
    .bind(fingerprint.as_slice())
    .fetch_optional(&mut **transaction)
    .await?;
    row.as_ref()
        .map(|row| decode_person(row, room_id, Some("participant_json")))
        .transpose()
}

fn decode_person(
    row: &SqliteRow,
    room_id: &str,
    participant_column: Option<&str>,
) -> Result<(HumanInvitePreflightPerson, bool), PersistenceError> {
    let user_id = row.try_get::<String, _>("user_id")?;
    let participant_id = row.try_get::<String, _>("participant_id")?;
    let profile_json = row
        .try_get::<Option<String>, _>("profile_json")?
        .ok_or_else(|| invalid_state("Stored human profile authority is missing."))?;
    let profile: UserProfile = serde_json::from_str(&profile_json)?;
    if profile.revision < 1 || user_id.is_empty() || participant_id.is_empty() {
        return Err(invalid_state("Stored human profile authority is invalid."));
    }
    let joined = match participant_column
        .map(|column| row.try_get::<Option<String>, _>(column))
        .transpose()?
        .flatten()
    {
        Some(json) => {
            let participant: Participant = serde_json::from_str(&json)?;
            if participant.room_id != room_id
                || participant.participant_id != participant_id
                || participant.participant_type != "human"
            {
                return Err(invalid_state("Stored human room membership is invalid."));
            }
            participant.status == ParticipantStatus::Joined
        }
        None => false,
    };
    Ok((
        HumanInvitePreflightPerson {
            operator: user_id == LOCAL_OPERATOR_USER_ID
                && participant_id == LOCAL_OPERATOR_PARTICIPANT_ID,
            participant_id,
            display_name: profile.display_name,
            avatar_image_url: profile.avatar_image_url,
        },
        joined,
    ))
}

fn invalid_state(message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: message.into(),
    }
}
