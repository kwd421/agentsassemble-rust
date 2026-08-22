use std::path::Path;

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use crate::{PersistenceError, SqliteStore, database_target::PreparedDatabase};

impl SqliteStore {
    /// Opens an explicit `SQLite` URL and verifies its ownership marker.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the store cannot be owned safely.
    pub async fn open(database_url: &str) -> Result<Self, PersistenceError> {
        Self::open_prepared(PreparedDatabase::from_url(database_url)?).await
    }

    /// Opens a file authority without interpreting path characters as URL options.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the path cannot be owned safely.
    pub async fn open_path(path: &Path) -> Result<Self, PersistenceError> {
        Self::open_prepared(PreparedDatabase::from_path(path)?).await
    }

    async fn open_prepared(prepared: PreparedDatabase) -> Result<Self, PersistenceError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(prepared.options.clone().create_if_missing(true))
            .await?;
        if prepared.identity.is_some() {
            acquire_file_authority_lock(&pool).await?;
        }
        prepared.revalidate()?;
        let store = Self {
            pool,
            _writer_lease: prepared.writer_lease,
            _database_identity: prepared.identity,
            created: prepared.created,
        };
        if store.created {
            store.initialize().await?;
        } else {
            store.verify_owner().await?;
        }
        Ok(store)
    }
}

async fn acquire_file_authority_lock(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut *connection)
        .await?;
    sqlx::query("COMMIT").execute(&mut *connection).await?;
    Ok(())
}
