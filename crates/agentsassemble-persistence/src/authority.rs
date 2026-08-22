use agentsassemble_domain::{
    AuthenticatedPrincipal, Participant, ParticipantStatus, Room, RoomStatus,
};
use sqlx::{Sqlite, Transaction};

use crate::PersistenceError;

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
    let room = load_active_room(transaction, &principal.room_id).await?;
    let participant =
        load_active_participant(transaction, &principal.room_id, &principal.participant_id).await?;
    if participant.room_id != principal.room_id
        || participant.participant_id != principal.participant_id
    {
        return Err(session_revoked());
    }
    Ok(room)
}

pub(crate) async fn load_active_participant(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    participant_id: &str,
) -> Result<Participant, PersistenceError> {
    load_active_room(transaction, room_id).await?;
    let participant_json = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(PersistenceError::ParticipantMissing)?;
    let participant: Participant = serde_json::from_str(&participant_json)?;
    if participant.room_id != room_id
        || participant.participant_id != participant_id
        || participant.status != ParticipantStatus::Joined
    {
        return Err(session_revoked());
    }
    Ok(participant)
}

async fn load_active_room(
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
