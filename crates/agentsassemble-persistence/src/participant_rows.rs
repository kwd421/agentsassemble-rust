use agentsassemble_domain::Participant;
use sqlx::SqliteConnection;

use crate::PersistenceError;

pub(crate) async fn load_participant_by_key(
    connection: &mut SqliteConnection,
    room_id: &str,
    participant_id: &str,
) -> Result<Option<Participant>, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(participant_id)
    .fetch_optional(connection)
    .await?;
    encoded
        .map(|value| serde_json::from_str(&value))
        .transpose()
        .map_err(Into::into)
}

pub(crate) async fn save_participant_exact(
    connection: &mut SqliteConnection,
    room_id: &str,
    participant_id: &str,
    participant: &Participant,
) -> Result<(), PersistenceError> {
    let changed = sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(participant)?)
    .bind(room_id)
    .bind(participant_id)
    .execute(connection)
    .await?;
    if changed.rows_affected() != 1 {
        return Err(PersistenceError::ParticipantMissing);
    }
    Ok(())
}
