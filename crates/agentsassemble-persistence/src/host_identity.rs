use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

/// Persistent private signing identity bound to one server authority.
///
/// This type deliberately implements neither `Debug` nor serialization so the
/// private key cannot enter generic diagnostics or wire projections.
pub struct PersistentHostIdentity {
    server_id: String,
    private_key_pkcs8: Vec<u8>,
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
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::{PersistenceError, SqliteStore};

    fn key_path(root: &std::path::Path) -> std::path::PathBuf {
        root.join("central-directory").join("host-ed25519.pk8")
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
