//! Member device lifetime and bounded retry history, owned by the admission transaction.
use crate::{PersistenceError, PreparedHumanAdmission};
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

pub(crate) const MAX_DEVICES: i64 = 8;
pub(crate) const MAX_TOMBSTONES: i64 = 32;

pub(crate) async fn retained_devices(
    tx: &mut Transaction<'_, Sqlite>,
    admission: &str,
    request: &PreparedHumanAdmission,
    now: DateTime<Utc>,
) -> Result<i64, PersistenceError> {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM human_room_sessions WHERE member_admission_id = ? AND state = 'active' AND expires_at > ? AND browser_credential_fingerprint != ?")
        .bind(admission).bind(now.timestamp_micros()).bind(request.browser_credential_fingerprint().as_slice())
        .fetch_one(&mut **tx).await?;
    Ok(count.min(MAX_DEVICES - 1))
}

pub(crate) async fn replace_devices(
    tx: &mut Transaction<'_, Sqlite>,
    admission: &str,
    request: &PreparedHumanAdmission,
    now: DateTime<Utc>,
) -> Result<Vec<[u8; 32]>, PersistenceError> {
    let replaced = sqlx::query("UPDATE human_room_sessions SET state = 'ended' WHERE member_admission_id = ? AND state = 'active' AND (expires_at <= ? OR browser_credential_fingerprint = ? OR admission_key IN (SELECT admission_key FROM human_room_sessions WHERE member_admission_id = ? AND state = 'active' AND expires_at > ? AND browser_credential_fingerprint != ? ORDER BY member_last_used_at DESC, admitted_at DESC, admission_key DESC LIMIT -1 OFFSET ?)) RETURNING session_fingerprint")
        .bind(admission).bind(now.timestamp_micros()).bind(request.browser_credential_fingerprint().as_slice())
        .bind(admission).bind(now.timestamp_micros()).bind(request.browser_credential_fingerprint().as_slice()).bind(MAX_DEVICES - 1)
        .fetch_all(&mut **tx).await?;
    replaced
        .into_iter()
        .map(|row| {
            crate::human_session_authority::fixed_session_fingerprint(
                row.try_get("session_fingerprint")?,
            )
        })
        .collect()
}

pub(crate) async fn prune(
    tx: &mut Transaction<'_, Sqlite>,
    admission: &str,
) -> Result<(), PersistenceError> {
    // A compact high-water mark fails closed for any challenge whose receipt was pruned.
    sqlx::query("UPDATE member_admissions SET replay_floor = MAX(replay_floor, COALESCE((SELECT MAX(member_challenge_expires_at) FROM (SELECT member_challenge_expires_at FROM human_room_sessions WHERE member_admission_id = ? AND state = 'ended' ORDER BY admitted_at DESC, admission_key DESC LIMIT -1 OFFSET ?)), 0)) WHERE admission_id = ?")
        .bind(admission).bind(MAX_TOMBSTONES).bind(admission).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM human_room_sessions WHERE admission_key IN (SELECT admission_key FROM human_room_sessions WHERE member_admission_id = ? AND state = 'ended' ORDER BY admitted_at DESC, admission_key DESC LIMIT -1 OFFSET ?)")
        .bind(admission).bind(MAX_TOMBSTONES).execute(&mut **tx).await?;
    Ok(())
}

pub(crate) async fn upgrade_v82(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    for ddl in [
        "ALTER TABLE human_room_sessions ADD COLUMN member_challenge BLOB CHECK(member_challenge IS NULL OR length(member_challenge) = 32)",
        "ALTER TABLE human_room_sessions ADD COLUMN member_challenge_expires_at INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE human_room_sessions ADD COLUMN member_last_used_at INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE member_admissions ADD COLUMN replay_floor INTEGER NOT NULL DEFAULT 0",
        "DROP INDEX human_room_sessions_active_participant_idx",
        "UPDATE human_room_sessions SET member_last_used_at = admitted_at WHERE member_admission_id IS NOT NULL",
    ] {
        sqlx::query(ddl).execute(&mut *tx).await?;
    }
    for ddl in crate::schema::INDEXES
        .iter()
        .filter(|ddl| ddl.contains(" ON human_room_sessions("))
    {
        sqlx::query(*ddl).execute(&mut *tx).await?;
    }
    sqlx::query("UPDATE human_room_sessions SET state = 'ended' WHERE member_admission_id IS NOT NULL AND expires_at <= ?")
        .bind(Utc::now().timestamp_micros()).execute(&mut *tx).await?;
    // Existing challenges are host-memory scoped and do not survive this restart.
    sqlx::query("DELETE FROM human_room_sessions WHERE admission_key IN (SELECT admission_key FROM (SELECT admission_key, ROW_NUMBER() OVER (PARTITION BY member_admission_id ORDER BY admitted_at DESC, admission_key DESC) AS position FROM human_room_sessions WHERE member_admission_id IS NOT NULL AND state = 'ended') WHERE position > ?)")
        .bind(MAX_TOMBSTONES).execute(&mut *tx).await?;
    sqlx::query("UPDATE runtime_metadata SET value = '83' WHERE key = 'schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}
