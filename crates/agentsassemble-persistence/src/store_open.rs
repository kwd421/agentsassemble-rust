use std::{path::Path, sync::Arc};

use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use crate::{
    PersistenceError, SqliteStore,
    database_target::PreparedDatabase,
    host_key_file::{HostKeyMaterial, HostKeyPolicy},
    schema_version::validate_schema_version,
    sqlite::SCHEMA_OWNER,
};

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
        let (fresh_authority, host_key_policy, initialization_nonce) =
            match inspect_database_authority(&pool).await? {
                DatabaseAuthority::Empty => {
                    let nonce = uuid::Uuid::new_v4().to_string();
                    install_initialization_marker(&pool, &nonce).await?;
                    (true, HostKeyPolicy::CreateOnly, nonce)
                }
                DatabaseAuthority::Initializing(nonce) => {
                    (true, HostKeyPolicy::CreateOrReuse, nonce)
                }
                DatabaseAuthority::Initialized(nonce) => (false, HostKeyPolicy::ReuseOnly, nonce),
            };
        let host_key = HostKeyMaterial::load_or_create(
            prepared.host_key_path.as_deref(),
            host_key_policy,
            &initialization_nonce,
        )?;
        let store = Self {
            pool,
            _writer_lease: prepared.writer_lease,
            _database_identity: prepared.identity,
            host_key: Arc::new(host_key),
            runtime_generation: format!("runtime-generation-v1-{}", uuid::Uuid::new_v4()).into(),
            created: fresh_authority,
        };
        if store.created {
            store.initialize().await?;
        } else {
            store.host_identity().await?;
        }
        Ok(store)
    }
}

enum DatabaseAuthority {
    Empty,
    Initializing(String),
    Initialized(String),
}

async fn inspect_database_authority(
    pool: &SqlitePool,
) -> Result<DatabaseAuthority, PersistenceError> {
    let object_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(pool)
    .await?;
    if object_count == 0 {
        return Ok(DatabaseAuthority::Empty);
    }
    let marker_table = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runtime_host_initialization'",
    )
    .fetch_one(pool)
    .await?;
    if object_count == 1 && marker_table == 1 {
        return load_initialization_nonce(pool)
            .await
            .map(DatabaseAuthority::Initializing);
    }
    verify_existing_authority(pool).await?;
    load_initialization_nonce(pool)
        .await
        .map(DatabaseAuthority::Initialized)
}

async fn install_initialization_marker(
    pool: &SqlitePool,
    nonce: &str,
) -> Result<(), PersistenceError> {
    let mut transaction = pool.begin().await?;
    sqlx::query(crate::schema::HOST_INITIALIZATION_DDL)
        .execute(&mut *transaction)
        .await?;
    sqlx::query("INSERT INTO runtime_host_initialization(singleton, nonce) VALUES (1, ?)")
        .bind(nonce)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(())
}

async fn load_initialization_nonce(pool: &SqlitePool) -> Result<String, PersistenceError> {
    let nonce = sqlx::query_scalar::<_, String>(
        "SELECT nonce FROM runtime_host_initialization WHERE singleton = 1",
    )
    .fetch_optional(pool)
    .await?
    .ok_or(PersistenceError::InvalidHostIdentity)?;
    let parsed =
        uuid::Uuid::parse_str(&nonce).map_err(|_| PersistenceError::InvalidHostIdentity)?;
    if parsed.to_string() != nonce {
        return Err(PersistenceError::InvalidHostIdentity);
    }
    Ok(nonce)
}

async fn verify_existing_authority(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let metadata_table = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'runtime_metadata'",
    )
    .fetch_one(pool)
    .await?;
    if metadata_table != 1 {
        return Err(PersistenceError::UnownedDatabase);
    }
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
    )
    .fetch_optional(pool)
    .await?;
    match owner {
        Some(owner) if owner == SCHEMA_OWNER => validate_schema_version(pool).await,
        Some(owner) => Err(PersistenceError::AuthorityConflict(owner)),
        None => Err(PersistenceError::UnownedDatabase),
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
