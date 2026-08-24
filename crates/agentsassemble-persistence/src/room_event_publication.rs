use agentsassemble_domain::RoomEvent;
use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

const PUBLICATION_BATCH_SIZE: i64 = 128;

impl SqliteStore {
    /// Loads the next contiguous durable events after the room's publication cursor.
    ///
    /// # Errors
    ///
    /// Fails if the room is absent, stored events are corrupt, or history is not contiguous.
    pub async fn pending_room_publications(
        &self,
        room_id: &str,
    ) -> Result<Vec<RoomEvent>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR IGNORE INTO room_event_publication_cursors(room_id, published_seq) SELECT room_id, 0 FROM rooms WHERE room_id = ?",
        )
        .bind(room_id)
        .execute(&mut *transaction)
        .await?;
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT published_seq FROM room_event_publication_cursors WHERE room_id = ?",
        )
        .bind(room_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(PersistenceError::RoomMissing)?;
        let rows = sqlx::query(
            "SELECT seq, event_json FROM room_events WHERE room_id = ? AND seq > ? ORDER BY seq LIMIT ?",
        )
        .bind(room_id)
        .bind(cursor)
        .bind(PUBLICATION_BATCH_SIZE)
        .fetch_all(&mut *transaction)
        .await?;
        let mut expected = cursor.saturating_add(1);
        let mut events = Vec::with_capacity(rows.len());
        for row in rows {
            let stored_seq = row.get::<i64, _>("seq");
            let event: RoomEvent =
                serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
            if stored_seq != expected || event.seq != stored_seq || event.room_id != room_id {
                return Err(invalid_publication_state());
            }
            events.push(event);
            expected = expected.saturating_add(1);
        }
        transaction.commit().await?;
        Ok(events)
    }

    /// Advances one room cursor after its exact next event has been offered to live receivers.
    ///
    /// # Errors
    ///
    /// Fails closed if another owner changed the cursor or the acknowledged event is not next.
    pub async fn acknowledge_room_publication(
        &self,
        room_id: &str,
        event_seq: i64,
    ) -> Result<(), PersistenceError> {
        if event_seq <= 0 {
            return Err(invalid_publication_state());
        }
        let updated = sqlx::query(
            "UPDATE room_event_publication_cursors SET published_seq = ? WHERE room_id = ? AND published_seq = ?",
        )
        .bind(event_seq)
        .bind(room_id)
        .bind(event_seq - 1)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(invalid_publication_state());
        }
        Ok(())
    }
}

fn invalid_publication_state() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "event_publication_state_invalid",
        message: "Durable room-event publication state is invalid.".to_owned(),
    }
}
