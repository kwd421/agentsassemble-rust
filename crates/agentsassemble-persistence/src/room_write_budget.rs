use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

use crate::PersistenceError;

pub(crate) const ROOM_WRITE_WINDOW_SECONDS: i64 = 60;
pub(crate) const MAX_ROOM_COMMANDS_PER_WINDOW: i64 = 14_400;
pub(crate) const MAX_ROOM_PAYLOAD_BYTES_PER_WINDOW: i64 = 32 * 1024 * 1024;

/// Returns the canonical byte charge used by socket and durable room budgets.
///
/// # Errors
///
/// Returns a serialization error when the command envelope cannot be encoded.
pub fn command_size(
    request_id: &str,
    action: &str,
    payload: &Value,
) -> Result<usize, PersistenceError> {
    Ok(serde_json::to_vec(&json!({
        "request_id": request_id,
        "action": action,
        "payload": payload,
    }))?
    .len())
}

pub(crate) async fn reserve_room_write_budget(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    payload_bytes: usize,
) -> Result<(), PersistenceError> {
    reserve_room_write_budget_with_limits(
        transaction,
        room_id,
        chrono::Utc::now().timestamp(),
        payload_bytes,
        MAX_ROOM_COMMANDS_PER_WINDOW,
        MAX_ROOM_PAYLOAD_BYTES_PER_WINDOW,
    )
    .await
}

async fn reserve_room_write_budget_with_limits(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    now_seconds: i64,
    payload_bytes: usize,
    command_limit: i64,
    payload_byte_limit: i64,
) -> Result<(), PersistenceError> {
    let payload_bytes = i64::try_from(payload_bytes).map_err(|_| budget_exceeded())?;
    if command_limit <= 0 || payload_byte_limit <= 0 || payload_bytes < 0 {
        return Err(budget_exceeded());
    }
    let window_started_at =
        now_seconds.div_euclid(ROOM_WRITE_WINDOW_SECONDS) * ROOM_WRITE_WINDOW_SECONDS;
    sqlx::query("DELETE FROM room_write_budgets WHERE window_started_at < ?")
        .bind(window_started_at)
        .execute(&mut **transaction)
        .await?;
    let existing = sqlx::query_as::<_, (i64, i64)>(
        "SELECT command_count, payload_bytes FROM room_write_budgets WHERE room_id = ? AND window_started_at = ?",
    )
    .bind(room_id)
    .bind(window_started_at)
    .fetch_optional(&mut **transaction)
    .await?;
    let (command_count, byte_count) = existing.unwrap_or((0, 0));
    if command_count.saturating_add(1) > command_limit
        || byte_count.saturating_add(payload_bytes) > payload_byte_limit
    {
        return Err(budget_exceeded());
    }
    sqlx::query(
        "INSERT INTO room_write_budgets(room_id, window_started_at, command_count, payload_bytes) VALUES (?, ?, 1, ?) ON CONFLICT(room_id, window_started_at) DO UPDATE SET command_count = room_write_budgets.command_count + 1, payload_bytes = room_write_budgets.payload_bytes + excluded.payload_bytes",
    )
    .bind(room_id)
    .bind(window_started_at)
    .bind(payload_bytes)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

fn budget_exceeded() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "write_budget_exceeded",
        message: "Room-wide authenticated write budget exceeded.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
    };
    use chrono::Utc;

    use super::reserve_room_write_budget_with_limits;
    use crate::SqliteStore;

    #[tokio::test]
    async fn durable_room_budget_cannot_be_sharded_by_principal_or_store_reopen() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let settings = RoomSettings::defaults("General");
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Operator".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        let store = SqliteStore::open_path_with_initial_room(&path, &room, &settings, &participant)
            .await
            .unwrap_or_else(|error| panic!("store: {error}"));
        let mut transaction = store
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("tx: {error}"));
        reserve_room_write_budget_with_limits(&mut transaction, "general", 1_000, 40, 2, 100)
            .await
            .unwrap_or_else(|error| panic!("first reservation: {error}"));
        transaction
            .commit()
            .await
            .unwrap_or_else(|error| panic!("commit: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen: {error}"));
        let mut transaction = reopened
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("tx: {error}"));
        reserve_room_write_budget_with_limits(&mut transaction, "general", 1_000, 40, 2, 100)
            .await
            .unwrap_or_else(|error| panic!("second reservation: {error}"));
        transaction
            .commit()
            .await
            .unwrap_or_else(|error| panic!("commit: {error}"));
        let mut transaction = reopened
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("tx: {error}"));
        let Err(error) =
            reserve_room_write_budget_with_limits(&mut transaction, "general", 1_000, 1, 2, 100)
                .await
        else {
            panic!("third room write must exceed the shared durable budget");
        };
        assert!(matches!(
            error,
            crate::PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                ..
            }
        ));
    }
}
