use chrono::Utc;
use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

const GENERATION_KEY: &str = "central_endpoint_generation";

impl SqliteStore {
    /// Advances the durable central endpoint generation monotonically.
    ///
    /// # Errors
    ///
    /// Fails when the stored generation is malformed, exhausted, or cannot be committed.
    pub async fn next_central_endpoint_generation(&self) -> Result<i64, PersistenceError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = ?")
            .bind(GENERATION_KEY)
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.get::<String, _>("value"));
        let current = stored
            .as_deref()
            .map(str::parse::<i64>)
            .transpose()
            .map_err(|_| invalid_generation())?
            .unwrap_or(0);
        let clock_floor = Utc::now().timestamp_millis().max(1);
        let next = current
            .checked_add(1)
            .ok_or_else(invalid_generation)?
            .max(clock_floor);
        sqlx::query(
            "INSERT INTO runtime_metadata(key, value) VALUES (?, ?) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(GENERATION_KEY)
        .bind(next.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(next)
    }
}

fn invalid_generation() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state".into(),
        message: "Stored central endpoint generation is invalid.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use crate::SqliteStore;

    #[tokio::test]
    async fn endpoint_generation_is_monotonic_across_store_reopen() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("open: {error}"));
        let first = store
            .next_central_endpoint_generation()
            .await
            .unwrap_or_else(|error| panic!("first: {error}"));
        drop(store);
        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen: {error}"));
        let second = reopened
            .next_central_endpoint_generation()
            .await
            .unwrap_or_else(|error| panic!("second: {error}"));
        assert!(second > first);
    }
}
