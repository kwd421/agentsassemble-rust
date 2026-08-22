use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 3;

impl SqliteStore {
    pub(crate) async fn migrate_schema(&self) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.get::<String, _>("value"));
        let version = match stored {
            Some(value) => value
                .parse::<i64>()
                .ok()
                .filter(|version| *version >= 1)
                .ok_or_else(|| PersistenceError::InvalidSchemaVersion(value.clone()))?,
            None => 1,
        };
        if version > CURRENT_SCHEMA_VERSION {
            return Err(PersistenceError::UnsupportedSchemaVersion {
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if version < 2 {
            sqlx::query(
                "CREATE TABLE agent_sessions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, session_json TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
            )
            .execute(&mut *transaction)
            .await?;
        }
        if version < 3 {
            redact_legacy_agent_results(&mut transaction).await?;
        }
        sqlx::query(
            "INSERT INTO runtime_metadata(key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(CURRENT_SCHEMA_VERSION.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn redact_legacy_agent_results(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), PersistenceError> {
    let rows =
        sqlx::query("SELECT rowid, result_json FROM command_results WHERE action = 'agent.create'")
            .fetch_all(&mut **transaction)
            .await?;
    for row in rows {
        let rowid = row.get::<i64, _>("rowid");
        let mut result: serde_json::Value =
            serde_json::from_str(row.get::<String, _>("result_json").as_str())?;
        if let Some(session) = result
            .get_mut("agent_session")
            .and_then(serde_json::Value::as_object_mut)
        {
            for private in [
                "workspace",
                "workspace_identity",
                "executable",
                "executable_identity",
                "runtime_profile_key",
            ] {
                session.remove(private);
            }
        }
        sqlx::query("UPDATE command_results SET result_json = ? WHERE rowid = ?")
            .bind(serde_json::to_string(&result)?)
            .bind(rowid)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::SqliteStore;

    #[tokio::test]
    async fn version_two_command_results_are_redacted_before_replay() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert legacy room: {error}"));
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES ('general', 'operator', 'request', 'agent.create', 'hash', ?)",
        )
        .bind(r#"{"agent_session":{"session_id":"agent-1","workspace":"/private","workspace_identity":"workspace-id","executable":"/bin/provider","executable_identity":"executable-id","runtime_profile_key":"profile"},"status":"created"}"#)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert legacy result: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '2' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set legacy version: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate legacy result: {error}"));
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT result_json FROM command_results WHERE request_id = 'request'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated result: {error}"));
        let result: serde_json::Value = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode migrated result: {error}"));
        assert_eq!(result["agent_session"]["session_id"], "agent-1");
        for private in [
            "workspace",
            "workspace_identity",
            "executable",
            "executable_identity",
            "runtime_profile_key",
        ] {
            assert!(result["agent_session"].get(private).is_none());
        }
    }

    #[tokio::test]
    async fn version_one_authority_is_upgraded_before_snapshots_use_sessions() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        sqlx::query("DROP TABLE agent_sessions")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v2 table: {error}"));
        sqlx::query("DELETE FROM runtime_metadata WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove version marker: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate store: {error}"));
        let table_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_sessions'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect table: {error}"));
        let version = sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key = 'schema_version'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect version: {error}"));
        assert_eq!(table_count, 1);
        assert_eq!(version, super::CURRENT_SCHEMA_VERSION.to_string());
    }

    #[tokio::test]
    async fn invalid_schema_version_is_not_reinterpreted_as_a_migratable_authority() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = 'corrupt' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("corrupt version marker: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(crate::PersistenceError::InvalidSchemaVersion(value)) if value == "corrupt"
        ));
    }
}
