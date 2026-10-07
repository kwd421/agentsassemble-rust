//! Durable display projection; canonical participants and rooms remain authority.
use crate::{PersistenceError, SqliteStore};
use serde::Serialize;
use sqlx::{Row, SqliteConnection};

pub(crate) const DDL: &str = "CREATE TABLE member_projection_outbox (
 binding_id TEXT NOT NULL REFERENCES central_identity_bindings(binding_id) ON DELETE RESTRICT,
 registration_epoch TEXT NOT NULL, projection_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','removed')),
 revision INTEGER NOT NULL CHECK(revision BETWEEN 1 AND 9007199254740991),
 acked_revision INTEGER NOT NULL DEFAULT 0, parked INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(binding_id, registration_epoch)) STRICT";
pub(crate) const SENDER_DDL: &str = "CREATE TABLE member_projection_sender (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), next_attempt_at INTEGER NOT NULL,
 failures INTEGER NOT NULL, day INTEGER NOT NULL, attempts INTEGER NOT NULL) STRICT";

#[derive(Clone, Serialize)]
pub struct MemberProjection {
    #[serde(skip)]
    pub binding_id: String,
    #[serde(skip)]
    pub registration_epoch: String,
    pub projection_id: String,
    pub state: String,
    pub revision: i64,
}

pub(crate) async fn anchor(
    c: &mut SqliteConnection,
    binding: &str,
    member: &crate::MemberAdmission,
    admission: bool,
) -> Result<(), PersistenceError> {
    let epoch = &member.registration_epoch;
    let projection = &member.projection_id;
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM member_projection_outbox WHERE binding_id=? AND registration_epoch=?)")
        .bind(binding).bind(epoch).fetch_one(&mut *c).await?;
    // Replacing an opaque anchor invalidates old ACKs without relabeling its revision.
    sqlx::query("INSERT INTO member_projection_outbox(binding_id,registration_epoch,projection_id,state,revision) VALUES (?,?,?,'removed',1) ON CONFLICT(binding_id,registration_epoch) DO UPDATE SET projection_id=excluded.projection_id, acked_revision=0, parked=CASE WHEN parked=2 THEN 2 ELSE 0 END WHERE projection_id != excluded.projection_id")
        .bind(binding).bind(epoch).bind(projection).execute(&mut *c).await?;
    recompute(c, binding, admission && exists).await
}

async fn recompute(
    c: &mut SqliteConnection,
    binding: &str,
    increment: bool,
) -> Result<(), PersistenceError> {
    sqlx::query("UPDATE member_projection_outbox SET state=CASE WHEN EXISTS(SELECT 1 FROM member_admissions m JOIN participants p ON p.room_id=m.room_id AND p.participant_id=m.participant_id JOIN rooms r ON r.room_id=m.room_id WHERE m.binding_id=? AND json_extract(p.participant_json,'$.status')='joined' AND json_extract(r.room_json,'$.status')='active') THEN 'active' ELSE 'removed' END, revision=revision+?, parked=CASE WHEN parked=2 OR ?=0 THEN parked ELSE 0 END WHERE binding_id=? AND registration_epoch=(SELECT value FROM runtime_metadata WHERE key='central_registration_epoch')")
        .bind(binding).bind(i64::from(increment)).bind(i64::from(increment)).bind(binding).execute(c).await?;
    Ok(())
}

pub(crate) async fn participant_changed(
    c: &mut SqliteConnection,
    room: &str,
    participant: &str,
) -> Result<(), PersistenceError> {
    let binding: Option<String> = sqlx::query_scalar(
        "SELECT binding_id FROM member_admissions WHERE room_id=? AND participant_id=?",
    )
    .bind(room)
    .bind(participant)
    .fetch_optional(&mut *c)
    .await?;
    if let Some(binding) = binding {
        recompute(c, &binding, true).await?;
    }
    Ok(())
}

pub(crate) async fn room_changed(
    c: &mut SqliteConnection,
    room: &str,
) -> Result<(), PersistenceError> {
    let bindings: Vec<String> =
        sqlx::query_scalar("SELECT DISTINCT binding_id FROM member_admissions WHERE room_id=?")
            .bind(room)
            .fetch_all(&mut *c)
            .await?;
    for binding in bindings {
        recompute(c, &binding, true).await?;
    }
    Ok(())
}

impl SqliteStore {
    /// Retains a redeemed opaque anchor even when existing membership denies admission.
    /// New bindings are created only by the successful admission transaction.
    /// # Errors
    /// Fails closed on stale redemption or persistence errors.
    pub async fn record_member_projection(
        &self,
        member: &crate::MemberAdmission,
        now: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        if !member.is_current(&mut tx, now).await? {
            return Err(crate::account_identity::rejected(
                "member_challenge_invalid",
                "Member challenge is no longer current.",
            ));
        }
        let binding: Option<String> = sqlx::query_scalar(
            "SELECT binding_id FROM central_identity_bindings WHERE issuer=? AND person_id=?",
        )
        .bind(&member.issuer)
        .bind(&member.person_id)
        .fetch_optional(&mut *tx)
        .await?;
        if let Some(binding) = binding {
            anchor(&mut tx, &binding, member, false).await?;
        }
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(())
    }

    /// Returns a due time only while current-epoch member projection work exists.
    /// Existing durable backoff and daily cap remain the delivery authority.
    /// # Errors
    /// Propagates database failures.
    pub async fn next_member_projection_attempt(
        &self,
        now: i64,
    ) -> Result<Option<i64>, PersistenceError> {
        let due = sqlx::query_scalar("SELECT MAX(COALESCE(s.next_attempt_at,0), CASE WHEN s.day=? AND s.attempts>=48 THEN (?+1)*86400 ELSE ? END) FROM (SELECT 1) LEFT JOIN member_projection_sender s ON s.singleton=1 WHERE EXISTS(SELECT 1 FROM member_projection_outbox WHERE parked=0 AND acked_revision<revision AND registration_epoch=(SELECT value FROM runtime_metadata WHERE key='central_registration_epoch'))")
            .bind(now.div_euclid(86400)).bind(now.div_euclid(86400)).bind(now).fetch_optional(&self.pool).await?;
        Ok(due)
    }

    /// Reserves one bounded report attempt durably, including crash/unknown outcomes.
    /// # Errors
    /// Returns persistence errors without sending a report.
    pub async fn take_member_projection_batch(
        &self,
        now: i64,
        jitter: u8,
    ) -> Result<Vec<MemberProjection>, PersistenceError> {
        // Event/timer wakeups only dispatch after this durable reservation is due.
        let due: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM member_projection_outbox WHERE parked=0 AND acked_revision<revision) AND NOT EXISTS(SELECT 1 FROM member_projection_sender WHERE singleton=1 AND (next_attempt_at>? OR (day=? AND attempts>=48)))")
            .bind(now).bind(now.div_euclid(86400)).fetch_one(&self.pool).await?;
        if !due {
            return Ok(vec![]);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE member_projection_outbox SET parked=2 WHERE parked!=2 AND registration_epoch != COALESCE((SELECT value FROM runtime_metadata WHERE key='central_registration_epoch'),'')")
            .execute(&mut *tx).await?;
        sqlx::query("INSERT OR IGNORE INTO member_projection_sender VALUES(1,0,0,0,0)")
            .execute(&mut *tx)
            .await?;
        let row = sqlx::query("SELECT * FROM member_projection_sender WHERE singleton=1")
            .fetch_one(&mut *tx)
            .await?;
        let day = now.div_euclid(86400);
        let attempts = if row.try_get::<i64, _>("day")? == day {
            row.try_get::<i64, _>("attempts")?
        } else {
            0
        };
        if row.try_get::<i64, _>("next_attempt_at")? > now || attempts >= 48 {
            tx.commit().await?;
            return Ok(vec![]);
        }
        let rows = sqlx::query("SELECT * FROM member_projection_outbox WHERE parked=0 AND acked_revision<revision ORDER BY registration_epoch,binding_id LIMIT 16")
            .fetch_all(&mut *tx).await?;
        let batch = rows
            .into_iter()
            .map(|r| {
                Ok(MemberProjection {
                    binding_id: r.try_get("binding_id")?,
                    registration_epoch: r.try_get("registration_epoch")?,
                    projection_id: r.try_get("projection_id")?,
                    state: r.try_get("state")?,
                    revision: r.try_get("revision")?,
                })
            })
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        if !batch.is_empty() {
            let failures = row.try_get::<i64, _>("failures")?.min(9);
            let delay =
                (60_i64 * (1_i64 << failures)).min(21600) * (80 + i64::from(jitter % 41)) / 100;
            sqlx::query("UPDATE member_projection_sender SET next_attempt_at=?, failures=?,day=?,attempts=? WHERE singleton=1")
                .bind(now+delay).bind(failures+1).bind(day).bind(attempts+1).execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(batch)
    }

    /// Acknowledges only the exact submitted incarnation, opaque anchor and revision.
    /// # Errors
    /// Returns database errors.
    pub async fn finish_member_projection_batch(
        &self,
        batch: &[MemberProjection],
        permanent: bool,
        now: i64,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        for item in batch {
            sqlx::query("UPDATE member_projection_outbox SET acked_revision=CASE WHEN ? THEN acked_revision ELSE ? END, parked=CASE WHEN ? THEN 1 ELSE parked END WHERE binding_id=? AND registration_epoch=? AND projection_id=? AND revision=? AND parked!=2")
                .bind(permanent).bind(item.revision).bind(permanent).bind(&item.binding_id).bind(&item.registration_epoch).bind(&item.projection_id).bind(item.revision).execute(&mut *tx).await?;
        }
        sqlx::query(
            "UPDATE member_projection_sender SET failures=0,next_attempt_at=? WHERE singleton=1",
        )
        .bind(now + 60)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(crate) async fn upgrade(
    pool: &sqlx::SqlitePool,
    version: &str,
) -> Result<(), PersistenceError> {
    if matches!(version, "84" | "85" | "86") {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    sqlx::query(DDL).execute(&mut *tx).await?;
    sqlx::query(SENDER_DDL).execute(&mut *tx).await?;
    sqlx::query("UPDATE runtime_metadata SET value='84' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "member_projection_tests.rs"]
mod tests;
