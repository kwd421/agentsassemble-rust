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
        let empty_authority = !prepared.created && database_is_empty(&pool).await?;
        let store = Self {
            pool,
            _writer_lease: prepared.writer_lease,
            _database_identity: prepared.identity,
            created: prepared.created || empty_authority,
        };
        if store.created {
            store.initialize().await?;
        } else {
            store.verify_owner().await?;
        }
        Ok(store)
    }
}

async fn database_is_empty(pool: &SqlitePool) -> Result<bool, PersistenceError> {
    let objects = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(pool)
    .await?;
    Ok(objects == 0)
}

async fn acquire_file_authority_lock(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut *connection)
        .await?;
    sqlx::query("COMMIT").execute(&mut *connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use crate::LocalBootstrapPhase;

    use super::SqliteStore;

    #[tokio::test]
    async fn interrupted_empty_file_retries_complete_bootstrap() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("create interrupted database file: {error}"));
        crate::private_fs::secure_file(&file)
            .unwrap_or_else(|error| panic!("secure interrupted database file: {error}"));
        drop(file);

        let restored = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("retry interrupted bootstrap: {error}"));
        assert!(restored.was_created());
        assert_eq!(
            restored
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("bootstrap status: {error}"))
                .phase,
            LocalBootstrapPhase::Empty
        );
        assert_eq!(
            restored
                .list_room_directory(true)
                .await
                .unwrap_or_else(|error| panic!("restored directory: {error}"))
                .len(),
            0
        );
        restored
            .bootstrap_local_authority("bdcb0e1f-667e-483b-a884-dd009cd3b138", "Operator")
            .await
            .unwrap_or_else(|error| panic!("complete local bootstrap: {error}"));
    }

    #[tokio::test]
    async fn fresh_authority_is_complete_with_zero_rooms_after_identity_bootstrap() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("initialize authority: {error}"));
        assert!(store.was_created());
        assert!(
            uuid::Uuid::parse_str(
                &store
                    .server_id()
                    .await
                    .unwrap_or_else(|error| panic!("server id: {error}"))
            )
            .is_ok()
        );
        let directory = store
            .list_room_directory(true)
            .await
            .unwrap_or_else(|error| panic!("room directory: {error}"));
        assert!(directory.is_empty());
        let request_id = "9cd99c6a-38fa-47d4-b048-918962d83ab5";
        let first = store
            .bootstrap_local_authority(request_id, "Local User")
            .await
            .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
        assert!(!first.deduplicated);
        assert_eq!(first.status.phase, LocalBootstrapPhase::Complete);
        assert_eq!(
            first
                .status
                .profile
                .as_ref()
                .map(|profile| profile.display_name.as_str()),
            Some("Local User")
        );
        assert!(
            store
                .list_room_directory(true)
                .await
                .unwrap_or_else(|error| panic!("room directory after bootstrap: {error}"))
                .is_empty()
        );
        let replay = store
            .bootstrap_local_authority(request_id, "Local User")
            .await
            .unwrap_or_else(|error| panic!("replay bootstrap: {error}"));
        assert!(replay.deduplicated);
        assert_eq!(replay.status, first.status);
        assert!(
            store
                .bootstrap_local_authority(request_id, "Different User")
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn concurrent_bootstrap_requests_have_one_authoritative_winner() {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("initialize authority: {error}"));
        let left_store = store.clone();
        let right_store = store.clone();
        let (left, right) = tokio::join!(
            left_store.bootstrap_local_authority("ea79d620-03f8-452f-bba4-086796c6abf8", "Left"),
            right_store.bootstrap_local_authority("49797ced-1401-4769-9041-ac172a0090a8", "Right")
        );

        assert_ne!(left.is_ok(), right.is_ok());
        let status = store
            .local_bootstrap_status()
            .await
            .unwrap_or_else(|error| panic!("inspect winning authority: {error}"));
        assert_eq!(status.phase, LocalBootstrapPhase::Complete);
        assert!(matches!(
            status
                .profile
                .as_ref()
                .map(|profile| profile.display_name.as_str()),
            Some("Left" | "Right")
        ));
    }

    #[tokio::test]
    async fn failed_bootstrap_transaction_leaves_authority_empty_for_exact_retry() {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("initialize authority: {error}"));
        sqlx::query(
            "CREATE TRIGGER reject_bootstrap_profile BEFORE INSERT ON user_profiles BEGIN SELECT RAISE(ABORT, 'injected bootstrap failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("install failure trigger: {error}"));
        let request_id = "1416e27f-72ea-4357-a366-c1449a0c2f4b";

        assert!(
            store
                .bootstrap_local_authority(request_id, "Retry User")
                .await
                .is_err()
        );
        assert_eq!(
            store
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("bootstrap status after rollback: {error}"))
                .phase,
            LocalBootstrapPhase::Empty
        );
        sqlx::query("DROP TRIGGER reject_bootstrap_profile")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove failure trigger: {error}"));
        assert_eq!(
            store
                .bootstrap_local_authority(request_id, "Retry User")
                .await
                .unwrap_or_else(|error| panic!("retry bootstrap: {error}"))
                .status
                .phase,
            LocalBootstrapPhase::Complete
        );
    }

    #[tokio::test]
    async fn schema_only_and_inconsistent_complete_authority_fail_closed() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("initialize schema: {error}"));
        assert_eq!(
            store
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("empty bootstrap status: {error}"))
                .phase,
            LocalBootstrapPhase::Empty
        );
        assert!(store.require_local_bootstrap_complete().await.is_err());
        store
            .bootstrap_local_authority("651d9765-9c65-47f6-8192-ce4871bce77e", "Operator")
            .await
            .unwrap_or_else(|error| panic!("complete authority: {error}"));
        sqlx::query(
            "UPDATE local_bootstrap_authority SET result_json = json_set(result_json, '$.status.profile.status', 'tampered') WHERE singleton = 1",
        )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("tamper immutable bootstrap profile: {error}"));
        assert_eq!(
            store
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("repair-required bootstrap status: {error}"))
                .phase,
            LocalBootstrapPhase::RepairRequired
        );
        assert!(store.require_local_bootstrap_complete().await.is_err());

        let empty = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("initialize empty authority: {error}"));
        sqlx::query(
            "UPDATE local_bootstrap_authority SET created_at = 'not-a-canonical-timestamp' WHERE singleton = 1",
        )
        .execute(&empty.pool)
        .await
        .unwrap_or_else(|error| panic!("tamper bootstrap creation metadata: {error}"));
        assert_eq!(
            empty
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("inspect tampered empty authority: {error}"))
                .phase,
            LocalBootstrapPhase::RepairRequired
        );
    }

    #[tokio::test]
    async fn empty_bootstrap_rejects_non_bootstrap_product_rows() {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("initialize empty authority: {error}"));
        sqlx::query(
            "INSERT INTO room_create_results(principal_id, request_id, payload_hash, result_json) VALUES ('orphan', 'orphan', 'orphan', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert non-bootstrap product row: {error}"));

        assert_eq!(
            store
                .local_bootstrap_status()
                .await
                .unwrap_or_else(|error| panic!("inspect partial authority: {error}"))
                .phase,
            LocalBootstrapPhase::RepairRequired
        );
        assert!(store.require_local_bootstrap_complete().await.is_err());
    }
}
