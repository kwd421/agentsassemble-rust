//! Durable mention delivery is independent of the Connector's process-local wait cursor.
use crate::{ConnectorSessionAuthorization, PersistenceError, RoomMutationAuthority, SqliteStore};
use agentsassemble_domain::{Participant, RoomEvent};
use serde_json::{Value, json};
use sqlx::{Sqlite, Transaction};

pub(crate) const DDL: &str = "CREATE TABLE connector_mentions (
    room_id TEXT NOT NULL, participant_id TEXT NOT NULL, seq INTEGER NOT NULL,
    PRIMARY KEY(room_id, participant_id, seq),
    FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE,
    FOREIGN KEY(room_id, seq) REFERENCES room_events(room_id, seq) ON DELETE CASCADE) STRICT";

pub(crate) async fn record(
    tx: &mut Transaction<'_, Sqlite>,
    event: &RoomEvent,
) -> Result<(), PersistenceError> {
    let rows = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id=?",
    )
    .bind(&event.room_id)
    .fetch_all(&mut **tx)
    .await?;
    let participants = rows
        .iter()
        .map(|row| serde_json::from_str::<Participant>(row))
        .collect::<Result<Vec<_>, _>>()?;
    for participant in &participants {
        if participant.participant_id != event.actor.participant_id
            && crate::room_turns::routing::mentions_target(
                &event.addressed_content(),
                participants
                    .iter()
                    .filter(|p| {
                        p.participant_type == "agent"
                            && p.participant_id != event.actor.participant_id
                    })
                    .map(|p| (p.participant_id.as_str(), p.display_name.as_str())),
                &participant.participant_id,
            )
            && active_connector(tx, &event.room_id, &participant.participant_id).await?
        {
            sqlx::query(
                "INSERT INTO connector_mentions(room_id,participant_id,seq) VALUES (?,?,?)",
            )
            .bind(&event.room_id)
            .bind(&participant.participant_id)
            .bind(event.seq)
            .execute(&mut **tx)
            .await?;
            trim(tx, &event.room_id, &participant.participant_id).await?;
        }
    }
    Ok(())
}

impl SqliteStore {
    /// Reads mode and unread mention information, acknowledging only messages actually returned by a read.
    ///
    /// # Errors
    /// Rejects stale Connector authority and propagates storage errors.
    pub async fn connector_mentions(
        &self,
        authorization: &ConnectorSessionAuthorization,
        read_ids: &[String],
        acknowledge_page: bool,
    ) -> Result<Value, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let authority = RoomMutationAuthority::ConnectorSession(authorization);
        let principal = authority.resolve(&mut tx).await?;
        let (_, settings) =
            crate::room_turns::support::load_active_room(&mut tx, &principal.room_id).await?;
        let rows = sqlx::query_scalar::<_, String>("SELECT e.event_json FROM connector_mentions m JOIN room_events e ON e.room_id=m.room_id AND e.seq=m.seq WHERE m.room_id=? AND m.participant_id=? ORDER BY m.seq LIMIT 50")
            .bind(&principal.room_id).bind(&principal.participant_id).fetch_all(&mut *tx).await?;
        let events = rows
            .iter()
            .map(|row| serde_json::from_str::<RoomEvent>(row))
            .collect::<Result<Vec<_>, _>>()?;
        let mentions = events.iter().map(|e| json!({"event_id":e.id,"by":e.actor.participant_id,"display_name":e.display_name})).collect::<Vec<_>>();
        let overflow: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM connector_mention_overflow WHERE room_id=? AND participant_id=?)")
            .bind(&principal.room_id).bind(&principal.participant_id).fetch_one(&mut *tx).await?;
        if acknowledge_page {
            for event in &events {
                sqlx::query(
                    "DELETE FROM connector_mentions WHERE room_id=? AND participant_id=? AND seq=?",
                )
                .bind(&principal.room_id)
                .bind(&principal.participant_id)
                .bind(event.seq)
                .execute(&mut *tx)
                .await?;
            }
            sqlx::query(
                "DELETE FROM connector_mention_overflow WHERE room_id=? AND participant_id=?",
            )
            .bind(&principal.room_id)
            .bind(&principal.participant_id)
            .execute(&mut *tx)
            .await?;
        }
        for id in read_ids {
            sqlx::query("DELETE FROM connector_mentions WHERE room_id=? AND participant_id=? AND seq=(SELECT seq FROM room_events WHERE room_id=? AND json_extract(event_json,'$.id')=?)")
                .bind(&principal.room_id).bind(&principal.participant_id).bind(&principal.room_id).bind(id).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(
            json!({"mention_overflow":overflow,"mention_resync_guidance":if overflow { "Older mentions were dropped. Resync room history before continuing." } else { "" },"unread_mentions":mentions,"mention_messages":events.iter().map(|event| agentsassemble_domain::public_event_for_principal(event, &principal)).collect::<Vec<_>>(),"conversation_mode":settings.conversation_mode,"guidance":if settings.conversation_mode == "free_access" {
            "This room does not notify you about ordinary messages. Visit when you have something to share or check; room_wait_next returns only when someone mentions you. Leave room_wait_next pending whenever you want to be reachable; a pending wait invokes no inference."
        } else { "" }}),
        )
    }
}

const BACKLOG: i64 = 200;

async fn active_connector(
    tx: &mut Transaction<'_, Sqlite>,
    room: &str,
    participant: &str,
) -> Result<bool, PersistenceError> {
    let active: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_connector_invites i JOIN participants p ON p.room_id=i.room_id AND p.participant_id=i.participant_id WHERE i.room_id=? AND i.participant_id=? AND i.revoked=0 AND i.session_fingerprint IS NOT NULL AND i.session_expires_at>? AND json_extract(p.participant_json,'$.status')='joined')")
        .bind(room).bind(participant).bind(chrono::Utc::now().timestamp_micros()).fetch_one(&mut **tx).await?;
    Ok(active)
}

async fn trim(
    tx: &mut Transaction<'_, Sqlite>,
    room: &str,
    participant: &str,
) -> Result<(), PersistenceError> {
    let removed = sqlx::query("DELETE FROM connector_mentions WHERE room_id=? AND participant_id=? AND seq IN (SELECT seq FROM connector_mentions WHERE room_id=? AND participant_id=? ORDER BY seq DESC LIMIT -1 OFFSET ?)")
        .bind(room).bind(participant).bind(room).bind(participant).bind(BACKLOG).execute(&mut **tx).await?;
    if removed.rows_affected() > 0 {
        sqlx::query(
            "INSERT OR IGNORE INTO connector_mention_overflow(room_id,participant_id) VALUES(?,?)",
        )
        .bind(room)
        .bind(participant)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn purge(
    tx: &mut Transaction<'_, Sqlite>,
    room: &str,
    participant: Option<&str>,
) -> Result<(), PersistenceError> {
    for table in ["connector_mentions", "connector_mention_overflow"] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {table} WHERE room_id=? AND (? IS NULL OR participant_id=?)"
        )))
        .bind(room)
        .bind(participant)
        .bind(participant)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

pub(crate) async fn upgrade(tx: &mut Transaction<'_, Sqlite>) -> Result<(), PersistenceError> {
    let participants = sqlx::query_as::<_, (String, String)>(
        "SELECT DISTINCT room_id,participant_id FROM connector_mentions",
    )
    .fetch_all(&mut **tx)
    .await?;
    for (room, participant) in participants {
        if active_connector(tx, &room, &participant).await? {
            trim(tx, &room, &participant).await?;
        } else {
            purge(tx, &room, Some(&participant)).await?;
        }
    }
    Ok(())
}
