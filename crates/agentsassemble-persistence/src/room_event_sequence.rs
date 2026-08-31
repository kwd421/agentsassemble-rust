use sqlx::{Sqlite, Transaction};

use crate::PersistenceError;

pub(crate) async fn next_sequence(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<i64, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?)
}
