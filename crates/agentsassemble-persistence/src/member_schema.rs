use crate::PersistenceError;
#[cfg(test)]
use sqlx::Row;
use sqlx::SqlitePool;

pub(crate) const DDL: &str = "CREATE TABLE member_admissions (
    binding_id TEXT NOT NULL REFERENCES central_identity_bindings(binding_id) ON DELETE RESTRICT,
    invite_id TEXT NOT NULL,
    admission_id TEXT NOT NULL UNIQUE,
    room_id TEXT NOT NULL,
    invite_scope TEXT NOT NULL,
    input_hash BLOB NOT NULL CHECK(length(input_hash) = 32),
    user_id TEXT NOT NULL,
    participant_id TEXT NOT NULL,
    session_key BLOB NOT NULL CHECK(length(session_key) = 32),
    result_json TEXT NOT NULL,
    replay_floor INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY(binding_id, invite_id),
    UNIQUE(binding_id, room_id)
) STRICT";

// v82 changes session provenance; v83 admits bounded simultaneous member devices.
// Older hosts reject this version; frozen binding DDL and bootstrap stay intact.
pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut connection = pool.acquire().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *connection)
            .await?;
    if version == "83" || version == "84" {
        return Ok(());
    }
    if version == "82" {
        drop(connection);
        return super::member_sessions::upgrade_v82(pool).await;
    }
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *connection)
        .await?;
    let result = async {
        let mut tx = sqlx::Connection::begin(&mut *connection).await?;
        if version != "81" {
            sqlx::query(crate::central_identity_bindings::TABLE_DDL).execute(&mut *tx).await?;
            sqlx::query(crate::central_identity_bindings::USER_INDEX_DDL).execute(&mut *tx).await?;
        }
        sqlx::query(DDL).execute(&mut *tx).await?;
        let ddl = crate::schema::TABLES.iter().find(|table| table.name == "human_room_sessions")
            .ok_or_else(|| PersistenceError::InvalidSchemaVersion("missing session DDL".into()))?.ddl;
        let replacement = ddl.replacen("human_room_sessions", "member_sessions_upgrade", 1);
        sqlx::raw_sql(sqlx::AssertSqlSafe(replacement)).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO member_sessions_upgrade (admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) SELECT admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state FROM human_room_sessions")
            .execute(&mut *tx).await?;
        sqlx::query("DROP TABLE human_room_sessions").execute(&mut *tx).await?;
        sqlx::query("ALTER TABLE member_sessions_upgrade RENAME TO human_room_sessions").execute(&mut *tx).await?;
        for index in crate::schema::INDEXES.iter().filter(|ddl| ddl.contains(" ON human_room_sessions(")) {
            sqlx::query(*index).execute(&mut *tx).await?;
        }
        if !sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *tx).await?.is_empty() {
            return Err(PersistenceError::InvalidSchemaVersion("member migration foreign key check failed".into()));
        }
        sqlx::query("UPDATE runtime_metadata SET value = '83' WHERE key = 'schema_version'").execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }.await;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *connection)
        .await?;
    result
}

#[cfg(test)]
pub(crate) async fn restore_v80_fixture(store: &crate::SqliteStore) -> Result<(), sqlx::Error> {
    let mut c = store.pool.acquire().await?;
    sqlx::query("PRAGMA foreign_keys = OFF")
        .execute(&mut *c)
        .await?;
    let mut tx = sqlx::Connection::begin(&mut *c).await?;
    let ddl =
        include_str!("member_v80_session.sql").replacen("human_room_sessions", "v80_sessions", 1);
    sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
        .execute(&mut *tx)
        .await?;
    let columns = sqlx::query("PRAGMA table_info(human_room_sessions)")
        .fetch_all(&mut *tx)
        .await?
        .iter()
        .map(|r| r.get::<String, _>("name"))
        .filter(|n| !n.starts_with("member_"))
        .collect::<Vec<_>>()
        .join(",");
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "INSERT INTO v80_sessions ({columns}) SELECT {columns} FROM human_room_sessions"
    )))
    .execute(&mut *tx)
    .await?;
    sqlx::query("DROP TABLE human_room_sessions")
        .execute(&mut *tx)
        .await?;
    sqlx::query("ALTER TABLE v80_sessions RENAME TO human_room_sessions")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DROP TABLE member_projection_outbox")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DROP TABLE member_projection_sender")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DROP TABLE member_admissions")
        .execute(&mut *tx)
        .await?;
    sqlx::query("DROP TABLE central_identity_bindings")
        .execute(&mut *tx)
        .await?;
    for index in crate::schema::INDEXES
        .iter()
        .filter(|s| s.contains(" ON human_room_sessions("))
    {
        if !index.contains("member_device_idx") && !index.contains("member_history_idx") {
            sqlx::raw_sql(sqlx::AssertSqlSafe(
                index.replace(" AND member_admission_id IS NULL", ""),
            ))
            .execute(&mut *tx)
            .await?;
        }
    }
    sqlx::query("UPDATE runtime_metadata SET value = '80' WHERE key = 'schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&mut *c)
        .await?;
    Ok(())
}
