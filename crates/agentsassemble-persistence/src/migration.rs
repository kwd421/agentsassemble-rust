use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 2;

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

#[cfg(test)]
mod tests {
    use crate::SqliteStore;

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
