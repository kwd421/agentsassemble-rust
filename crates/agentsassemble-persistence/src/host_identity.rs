use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

/// Persistent private signing identity bound to one server authority.
///
/// This type deliberately implements neither `Debug` nor serialization so the
/// private key cannot enter generic diagnostics or wire projections.
pub struct PersistentHostIdentity {
    server_id: String,
    private_key_pkcs8: Vec<u8>,
    session_hmac_key: [u8; 32],
}

impl PersistentHostIdentity {
    #[must_use]
    pub fn server_id(&self) -> &str {
        &self.server_id
    }

    #[must_use]
    pub fn private_key_pkcs8(&self) -> &[u8] {
        &self.private_key_pkcs8
    }

    #[must_use]
    pub fn session_hmac_key(&self) -> &[u8; 32] {
        &self.session_hmac_key
    }
}

impl SqliteStore {
    /// Loads the exact file-backed Ed25519 private key bound to this server ID.
    ///
    /// # Errors
    ///
    /// Rejects missing, malformed, or differently bound host identity state.
    pub async fn host_identity(&self) -> Result<PersistentHostIdentity, PersistenceError> {
        let row = sqlx::query(
            "SELECT host.server_id, host.public_key, metadata.value AS expected_server_id
             FROM runtime_host_identity AS host
             JOIN runtime_metadata AS metadata ON metadata.key = 'server_id'
             WHERE host.singleton = 1",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PersistenceError::InvalidHostIdentity)?;
        let server_id = row.get::<String, _>("server_id");
        let expected_server_id = row.get::<String, _>("expected_server_id");
        if server_id != expected_server_id || uuid::Uuid::parse_str(&server_id).is_err() {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        let public_key = row.get::<Vec<u8>, _>("public_key");
        if public_key.as_slice() != self.host_key.public_key() {
            return Err(PersistenceError::InvalidHostIdentity);
        }
        Ok(PersistentHostIdentity {
            server_id,
            private_key_pkcs8: self.host_key.private_key_pkcs8().to_vec(),
            session_hmac_key: *self.host_key.session_hmac_key(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

    use crate::{
        PersistenceError, SqliteStore,
        host_key_file::{HostKeyMaterial, HostKeyPolicy},
    };

    fn key_path(root: &std::path::Path) -> std::path::PathBuf {
        root.join("central-directory").join("host-ed25519.pk8")
    }

    async fn marker_only_database(root: &std::path::Path, nonce: &str) -> std::path::PathBuf {
        let path = root.join("runtime.sqlite3");
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("create marker database: {error}"));
        crate::private_fs::secure_file(&file)
            .unwrap_or_else(|error| panic!("secure marker database: {error}"));
        drop(file);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(SqliteConnectOptions::new().filename(&path))
            .await
            .unwrap_or_else(|error| panic!("open marker database: {error}"));
        sqlx::query(crate::schema::HOST_INITIALIZATION_DDL)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("create host marker: {error}"));
        sqlx::query("INSERT INTO runtime_host_initialization(singleton, nonce) VALUES (1, ?)")
            .bind(nonce)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("insert host marker: {error}"));
        pool.close().await;
        path
    }

    #[tokio::test]
    async fn host_identity_remains_bound_to_the_server_across_reopen() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let first = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create authority: {error}"));
        let first_identity = first
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load first host identity: {error}"));
        let first_server_id = first_identity.server_id().to_owned();
        let first_private_key = first_identity.private_key_pkcs8().to_vec();
        let first_session_hmac_key = *first_identity.session_hmac_key();
        drop(first_identity);
        drop(first);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen authority: {error}"));
        let reopened_identity = reopened
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load reopened host identity: {error}"));
        assert_eq!(reopened_identity.server_id(), first_server_id);
        assert!(
            reopened_identity.private_key_pkcs8() == first_private_key,
            "host private key changed across reopen"
        );
        assert!(
            reopened_identity.session_hmac_key() == &first_session_hmac_key,
            "session HMAC key changed across reopen"
        );

        let other_directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("other tempdir: {error}"));
        let other_database = other_directory.path().join("runtime.sqlite3");
        let other = SqliteStore::open_path(&other_database)
            .await
            .unwrap_or_else(|error| panic!("create other authority: {error}"));
        let other_identity = other
            .host_identity()
            .await
            .unwrap_or_else(|error| panic!("load other host identity: {error}"));
        assert!(
            other_identity.session_hmac_key() != &first_session_hmac_key,
            "fresh hosts shared a session HMAC key"
        );
    }

    #[tokio::test]
    async fn invalid_session_hmac_envelopes_fail_closed_without_rewrite() {
        for corruption in ["missing", "short", "noncanonical", "old-version"] {
            let directory =
                tempfile::tempdir().unwrap_or_else(|error| panic!("{corruption} tempdir: {error}"));
            let database = directory.path().join("runtime.sqlite3");
            let store = SqliteStore::open_path(&database)
                .await
                .unwrap_or_else(|error| panic!("create {corruption} authority: {error}"));
            drop(store);

            let key_file = key_path(directory.path());
            let original = std::fs::read(&key_file)
                .unwrap_or_else(|error| panic!("read {corruption} envelope: {error}"));
            let mut value: serde_json::Value = serde_json::from_slice(&original)
                .unwrap_or_else(|error| panic!("decode {corruption} envelope: {error}"));
            let object = value
                .as_object_mut()
                .unwrap_or_else(|| panic!("{corruption} envelope must be an object"));
            match corruption {
                "missing" => {
                    object.remove("session_hmac_key");
                }
                "short" => {
                    object.insert("session_hmac_key".to_owned(), "AA".into());
                }
                "noncanonical" => {
                    object.insert("session_hmac_key".to_owned(), "AA==".into());
                }
                "old-version" => {
                    object.insert("version".to_owned(), 1.into());
                }
                _ => unreachable!("bounded corruption cases"),
            }
            let corrupted = serde_json::to_vec(&value)
                .unwrap_or_else(|error| panic!("encode {corruption} envelope: {error}"));
            std::fs::write(&key_file, &corrupted)
                .unwrap_or_else(|error| panic!("write {corruption} envelope: {error}"));

            assert!(matches!(
                SqliteStore::open_path(&database).await,
                Err(PersistenceError::InvalidHostIdentity)
            ));
            let after = std::fs::read(&key_file)
                .unwrap_or_else(|error| panic!("reread {corruption} envelope: {error}"));
            assert!(
                after == corrupted,
                "{corruption} envelope was rewritten after fail-closed load"
            );
        }
    }

    #[tokio::test]
    async fn differently_bound_host_identity_rejects_the_current_schema() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create authority: {error}"));
        sqlx::query("UPDATE runtime_host_identity SET server_id = ? WHERE singleton = 1")
            .bind(uuid::Uuid::new_v4().to_string())
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("substitute host server id: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(PersistenceError::InvalidHostIdentity)
        ));
    }

    #[tokio::test]
    async fn database_only_backup_cannot_clone_the_host_signing_authority() {
        let source = tempfile::tempdir().unwrap_or_else(|error| panic!("source tempdir: {error}"));
        let source_database = source.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&source_database)
            .await
            .unwrap_or_else(|error| panic!("create source authority: {error}"));
        drop(store);

        let private_key = key_path(source.path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mode = private_key
                .metadata()
                .unwrap_or_else(|error| panic!("inspect host key permissions: {error}"))
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0);
        }

        let clone = tempfile::tempdir().unwrap_or_else(|error| panic!("clone tempdir: {error}"));
        let cloned_database = clone.path().join("runtime.sqlite3");
        std::fs::copy(&source_database, &cloned_database)
            .unwrap_or_else(|error| panic!("copy database backup: {error}"));
        assert!(matches!(
            SqliteStore::open_path(&cloned_database).await,
            Err(PersistenceError::HostIdentityMissing)
        ));
        assert!(!clone.path().join("central-directory").exists());
    }

    #[tokio::test]
    async fn orphaned_host_key_never_rebinds_to_a_new_server_identity() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let database = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&database)
            .await
            .unwrap_or_else(|error| panic!("create authority: {error}"));
        drop(store);
        std::fs::remove_file(&database)
            .unwrap_or_else(|error| panic!("remove test database: {error}"));
        assert!(key_path(directory.path()).is_file());

        for attempt in 1..=2 {
            assert!(matches!(
                SqliteStore::open_path(&database).await,
                Err(PersistenceError::InvalidHostIdentity)
            ));
            assert!(
                !database.exists(),
                "attempt {attempt} must not create an empty database that enables key reuse"
            );
        }
    }

    #[tokio::test]
    async fn matching_marker_and_key_resume_one_interrupted_initialization() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let nonce = uuid::Uuid::new_v4().to_string();
        let database = marker_only_database(directory.path(), &nonce).await;
        drop(
            HostKeyMaterial::load_or_create(
                Some(&key_path(directory.path())),
                HostKeyPolicy::CreateOnly,
                &nonce,
            )
            .unwrap_or_else(|error| panic!("create matching host key: {error}")),
        );

        let restored = SqliteStore::open_path(&database)
            .await
            .unwrap_or_else(|error| panic!("resume marker-bound initialization: {error}"));
        assert!(restored.was_created());
        drop(restored);
        assert!(
            !SqliteStore::open_path(&database)
                .await
                .unwrap_or_else(|error| panic!("reopen restored authority: {error}"))
                .was_created()
        );
    }

    #[tokio::test]
    async fn mismatched_marker_and_key_remain_rejected_across_reopen() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let marker_nonce = uuid::Uuid::new_v4().to_string();
        let stale_nonce = uuid::Uuid::new_v4().to_string();
        let database = marker_only_database(directory.path(), &marker_nonce).await;
        drop(
            HostKeyMaterial::load_or_create(
                Some(&key_path(directory.path())),
                HostKeyPolicy::CreateOnly,
                &stale_nonce,
            )
            .unwrap_or_else(|error| panic!("create stale host key: {error}")),
        );

        for attempt in 1..=2 {
            assert!(matches!(
                SqliteStore::open_path(&database).await,
                Err(PersistenceError::InvalidHostIdentity)
            ));
            assert!(
                database.is_file(),
                "attempt {attempt} lost its durable marker"
            );
        }
    }

    #[tokio::test]
    async fn substituted_private_key_rejects_the_database_public_binding() {
        let first = tempfile::tempdir().unwrap_or_else(|error| panic!("first tempdir: {error}"));
        let second = tempfile::tempdir().unwrap_or_else(|error| panic!("second tempdir: {error}"));
        let first_database = first.path().join("runtime.sqlite3");
        let second_database = second.path().join("runtime.sqlite3");
        drop(
            SqliteStore::open_path(&first_database)
                .await
                .unwrap_or_else(|error| panic!("create first authority: {error}")),
        );
        drop(
            SqliteStore::open_path(&second_database)
                .await
                .unwrap_or_else(|error| panic!("create second authority: {error}")),
        );

        std::fs::copy(key_path(second.path()), key_path(first.path()))
            .unwrap_or_else(|error| panic!("substitute host key: {error}"));
        assert!(matches!(
            SqliteStore::open_path(&first_database).await,
            Err(PersistenceError::InvalidHostIdentity)
        ));
    }
}
