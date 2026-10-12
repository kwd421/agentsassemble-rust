//! Acceptance-time delivery facts are independent of mutable room settings.
use crate::{ConnectorSessionAuthorization, PersistenceError, RoomMutationAuthority, SqliteStore};
use agentsassemble_domain::RoomEvent;
use sqlx::{Row, Sqlite, Transaction};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS connector_message_delivery (
    room_id TEXT NOT NULL, seq INTEGER NOT NULL,
    ordinary INTEGER NOT NULL CHECK(ordinary IN (0,1)),
    PRIMARY KEY(room_id,seq),
    FOREIGN KEY(room_id,seq) REFERENCES room_events(room_id,seq) ON DELETE CASCADE) STRICT";
pub(crate) const OVERFLOW_DDL: &str = "CREATE TABLE IF NOT EXISTS connector_mention_overflow (
    room_id TEXT NOT NULL, participant_id TEXT NOT NULL,
    PRIMARY KEY(room_id,participant_id),
    FOREIGN KEY(room_id,participant_id) REFERENCES participants(room_id,participant_id) ON DELETE CASCADE) STRICT";

pub(crate) async fn record(
    tx: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    if event.event_type == "message_final" {
        sqlx::query("INSERT INTO connector_message_delivery(room_id,seq,ordinary) SELECT room_id,?,json_extract(settings_json,'$.conversation_mode')!='free_access' FROM rooms WHERE room_id=?")
            .bind(event.seq).bind(&event.room_id).execute(&mut **tx).await?;
    }
    Ok(())
}

impl SqliteStore {
    /// Return ordinary notification sequences using only persisted acceptance facts.
    /// # Errors
    /// Rejects stale authority and requires resync for pre-migration unclassified history.
    pub async fn connector_ordinary_messages(
        &self,
        authorization: &ConnectorSessionAuthorization,
        sequences: &[i64],
    ) -> Result<Vec<i64>, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let principal = RoomMutationAuthority::ConnectorSession(authorization)
            .resolve(&mut tx)
            .await?;
        let rows = sqlx::query("SELECT wanted.value AS seq,d.ordinary FROM json_each(?) wanted LEFT JOIN connector_message_delivery d ON d.room_id=? AND d.seq=wanted.value")
            .bind(serde_json::to_string(sequences)?).bind(&principal.room_id).fetch_all(&mut *tx).await?;
        let mut ordinary = Vec::new();
        for row in rows {
            match row.get::<Option<bool>, _>("ordinary") {
                Some(true) => ordinary.push(row.get("seq")),
                Some(false) => {}
                None => {
                    return Err(crate::connector_session::rejected(
                        "connector_resync_required",
                        "Read room history to resync messages accepted before delivery classification.",
                    ));
                }
            }
        }
        tx.commit().await?;
        Ok(ordinary)
    }
}
