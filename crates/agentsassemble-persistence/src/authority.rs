use agentsassemble_domain::{
    AuthenticatedPrincipal, Participant, ParticipantStatus, Room, RoomStatus,
};
use sqlx::{Sqlite, Transaction};

use crate::{HumanSessionAuthorization, PersistenceError};

/// Explicit provenance for a room mutation; never serialized into the public principal.
#[derive(Clone, Copy)]
pub enum RoomMutationAuthority<'a> {
    /// Principal already authenticated by the native or bridge transport owner.
    TrustedPrincipal(&'a AuthenticatedPrincipal),
    /// Durable browser session, revalidated inside the mutation transaction.
    HumanSession(&'a HumanSessionAuthorization),
    /// Exact current-conversation AI session, revalidated in the transaction.
    ConnectorSession(&'a crate::ConnectorSessionAuthorization),
    /// Exact device-bound remote operator session, revalidated in the transaction.
    OperatorSession(&'a crate::OperatorSessionAuthorization),
}

impl<'a> RoomMutationAuthority<'a> {
    pub(crate) const fn principal(self) -> &'a AuthenticatedPrincipal {
        match self {
            Self::TrustedPrincipal(principal) => principal,
            Self::HumanSession(session) => session.principal(),
            Self::OperatorSession(session) => session.principal(),
            Self::ConnectorSession(session) => session.principal(),
        }
    }

    pub(crate) async fn resolve(
        self,
        transaction: &mut Transaction<'_, Sqlite>,
    ) -> Result<std::borrow::Cow<'a, AuthenticatedPrincipal>, PersistenceError> {
        match self {
            Self::TrustedPrincipal(principal) => Ok(std::borrow::Cow::Borrowed(principal)),
            Self::ConnectorSession(expected) => {
                let current = crate::connector_session::revalidate_in(
                    transaction,
                    expected,
                    chrono::Utc::now(),
                )
                .await?;
                Ok(std::borrow::Cow::Owned(current.principal().clone()))
            }
            Self::OperatorSession(expected) => {
                let current = crate::operator_pairing::revalidate_operator_session(
                    transaction,
                    expected,
                    chrono::Utc::now(),
                )
                .await?;
                Ok(std::borrow::Cow::Owned(current.principal().clone()))
            }
            Self::HumanSession(expected) => {
                let (current, _) = crate::human_session_authority::revalidate_human_session(
                    transaction,
                    expected,
                    chrono::Utc::now(),
                )
                .await?;
                Ok(std::borrow::Cow::Owned(current.principal().clone()))
            }
        }
    }
}

pub(crate) async fn authorize_session(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
) -> Result<(), PersistenceError> {
    active_room_for_principal(transaction, principal).await?;
    Ok(())
}

pub(crate) async fn active_room_for_principal(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
) -> Result<Room, PersistenceError> {
    let (room, _) =
        load_active_membership(transaction, &principal.room_id, &principal.participant_id).await?;
    Ok(room)
}

pub(crate) async fn load_active_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    let (_, participant) = load_active_membership(transaction, room_id, participant_id).await?;
    Ok(participant)
}

pub(crate) async fn load_active_membership(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<(Room, Participant), PersistenceError> {
    let room = load_active_room(transaction, room_id).await?;
    let participant =
        crate::participant_rows::load_participant_by_key(transaction, room_id, participant_id)
            .await?
            .ok_or(PersistenceError::ParticipantMissing)?;
    if participant.room_id != room_id
        || participant.participant_id != participant_id
        || participant.status != ParticipantStatus::Joined
    {
        return Err(session_revoked());
    }
    Ok((room, participant))
}

pub(crate) async fn load_active_room(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<Room, PersistenceError> {
    let room_json =
        sqlx::query_scalar::<_, String>("SELECT room_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or(PersistenceError::RoomMissing)?;
    let room: Room = serde_json::from_str(&room_json)?;
    if room.room_id != room_id || room.status != RoomStatus::Active {
        return Err(PersistenceError::CommandRejected {
            code: "room_inactive",
            message: "Closed or archived rooms do not accept active sessions.".to_owned(),
        });
    }
    Ok(room)
}

fn session_revoked() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "session_revoked",
        message: "This room session has ended.".to_owned(),
    }
}
