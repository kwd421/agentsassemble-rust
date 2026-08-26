use sqlx::{Row, SqlitePool};

use crate::PersistenceError;

pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 39;

pub(crate) async fn validate_schema_version(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
        .fetch_optional(pool)
        .await?
        .map(|row| row.get::<String, _>("value"))
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion("missing".to_owned()))?;
    let found = stored
        .parse::<i64>()
        .ok()
        .filter(|version| *version >= 1)
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion(stored.clone()))?;
    if found != CURRENT_SCHEMA_VERSION {
        return Err(PersistenceError::SchemaVersionMismatch {
            found,
            required: CURRENT_SCHEMA_VERSION,
        });
    }
    let server_id = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'server_id'",
    )
    .fetch_optional(pool)
    .await?
    .ok_or(PersistenceError::InvalidServerId)?;
    uuid::Uuid::parse_str(&server_id)
        .map(|_| ())
        .map_err(|_| PersistenceError::InvalidServerId)
}

#[cfg(test)]
mod tests {
    use super::CURRENT_SCHEMA_VERSION;
    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn older_schema_is_rejected_without_conversion() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create current store: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind((CURRENT_SCHEMA_VERSION - 1).to_string())
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set older schema: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(PersistenceError::SchemaVersionMismatch { found, required })
                if found == CURRENT_SCHEMA_VERSION - 1 && required == CURRENT_SCHEMA_VERSION
        ));
    }
}
