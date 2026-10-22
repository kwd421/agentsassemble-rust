//! Only freshly reconstructed data receives a new host identity. No listener,
//! provider, scheduler or ordinary runtime is started by this owner.
use crate::{
    PersistenceError,
    backup_codec::BackupManifest,
    backup_json::invalid,
    host_key_file::{HostKeyMaterial, HostKeyPolicy},
    restore_job::{RestoreJob, RestorePhase},
};
use sqlx::{Connection, Row, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::Path;

impl RestoreJob {
    /// Check the destination volume before decoding any payload from the same
    /// stream whose manifest is supplied here. Headroom covers decoded + rebuilt
    /// DBs, rollback journal, WAL and two DB equivalents for rebuilt FTS/indexes;
    /// a fixed margin covers small DBs and journal/key metadata. Concurrent volume
    /// use can still fail writes, so cancellation must require no new allocation.
    /// # Errors
    /// Rejects insufficient capacity or a failed filesystem capacity query.
    pub fn check_decode_space(&self, manifest: &BackupManifest) -> std::io::Result<()> {
        require_decode_space(manifest.database_bytes, fs2::available_space(&self.stage)?)
    }

    /// Validate the complete payload and provision a fresh host/session identity.
    /// # Errors
    /// Refuses changed bytes or inconsistent manifest/data. Validating retries rebuild
    /// only this job's incomplete generated files from the verified input.
    pub async fn prepare(
        &mut self,
        input: &Path,
        manifest: BackupManifest,
    ) -> Result<(), PersistenceError> {
        if self.journal.phase != RestorePhase::Validating || self.journal.manifest.is_some() {
            return Err(invalid());
        }
        manifest.validate().map_err(|_| invalid())?;
        let mut input_file = std::fs::File::open(input).map_err(PersistenceError::WriterLease)?;
        let (size, hash) =
            crate::backup_codec::measure_payload(&mut input_file).map_err(|_| invalid())?;
        if size != manifest.database_bytes || hash != manifest.database_sha256 {
            return Err(invalid());
        }
        self.clear_incomplete_preparation()?;
        let database = self.stage.join("runtime.sqlite3");
        crate::backup_database::reconstruct_payload(input, &database, manifest.schema_version)
            .await?;
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database))
                .await?;
        let provisioned = provision_identity(&mut connection, &self.stage, &manifest).await;
        #[cfg(test)]
        if provisioned.is_ok() && self.fail_upgrade {
            // Fail the real 88 -> 89 transaction after 87 -> 88 committed.
            sqlx::raw_sql("CREATE TRIGGER fail_upgrade BEFORE UPDATE ON runtime_metadata WHEN NEW.key='schema_version' AND NEW.value='89' BEGIN SELECT RAISE(ABORT,'injected migration failure'); END")
                .execute(&mut connection).await?;
        }
        let closed = connection.close().await;
        provisioned?;
        closed?;
        // Ordinary opening verifies the freshly provisioned identity before the
        // normal upgrade_schema path. No listeners or central requests run here.
        let store = crate::SqliteStore::open_path(&database).await?;
        let validated = rebuild_and_validate(&store, &manifest).await;
        let closed = store.close().await;
        validated?;
        closed?;
        self.preflight_activation()?;
        #[cfg(unix)]
        for directory in [&self.stage, &self.stage.join("central-directory")] {
            crate::restore_job::sync_directory(directory)?;
        }
        self.journal.manifest = Some(manifest);
        self.journal.phase = RestorePhase::Prepared;
        self.save()
    }

    fn clear_incomplete_preparation(&self) -> Result<(), PersistenceError> {
        // Only Validating calls this, after revalidating the original payload.
        // These are generated files of this exclusively leased job, never input
        // bytes, live installation files, or a Prepared/SwitchUnknown identity.
        let keys = self.stage.join("central-directory");
        match std::fs::symlink_metadata(&keys) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(PersistenceError::WriterLease(error)),
        }
        for name in [
            "runtime.sqlite3",
            "runtime.sqlite3-wal",
            "runtime.sqlite3-shm",
            "runtime.sqlite3-journal",
            "runtime.sqlite3.writer.lock",
            "activation-link-probe",
            "central-directory/host-ed25519.pk8",
        ] {
            match std::fs::remove_file(self.stage.join(name)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(PersistenceError::WriterLease(error)),
            }
        }
        Ok(())
    }

    fn preflight_activation(&self) -> Result<(), PersistenceError> {
        let parent = self.target.parent().ok_or_else(invalid)?;
        let key_directory = parent.join("central-directory");
        match std::fs::symlink_metadata(&key_directory) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(invalid()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                std::fs::create_dir(&key_directory).map_err(PersistenceError::WriterLease)?;
            }
            Err(error) => return Err(PersistenceError::WriterLease(error)),
        }
        crate::private_fs::secure_private_directory(&key_directory)
            .map_err(PersistenceError::WriterLease)?;
        let probe = tempfile::NamedTempFile::new_in(&key_directory)
            .map_err(PersistenceError::WriterLease)?;
        let link = self.stage.join("activation-link-probe");
        std::fs::hard_link(probe.path(), &link).map_err(PersistenceError::WriterLease)?;
        std::fs::remove_file(link).map_err(PersistenceError::WriterLease)?;
        Ok(())
    }

    /// Return the prepared private identity solely to the native central signer.
    /// # Errors
    /// Rejects an unprepared job or changed key/database binding.
    pub async fn host_identity(&self) -> Result<crate::PersistentHostIdentity, PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::Prepared | RestorePhase::SwitchUnknown | RestorePhase::Committed
        ) {
            return Err(invalid());
        }
        let database = self.stage.join("runtime.sqlite3");
        // Cancellation can remove data while the original journal still exists.
        // A key read must not initialize a replacement installation in staging.
        if !database
            .try_exists()
            .map_err(PersistenceError::WriterLease)?
        {
            return Err(invalid());
        }
        let store = crate::SqliteStore::open_path(&database).await?;
        let identity = store.host_identity().await?;
        store.close().await?;
        Ok(identity)
    }
}

async fn provision_identity(
    connection: &mut SqliteConnection,
    stage: &Path,
    manifest: &BackupManifest,
) -> Result<(), PersistenceError> {
    let server_id: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='server_id'")
            .fetch_one(&mut *connection)
            .await?;
    if server_id != manifest.server_id
        || crate::archive_schema::version(connection).await? != manifest.schema_version
        || counts(connection).await? != (manifest.rooms, manifest.messages, manifest.assets)
    {
        return Err(invalid());
    }
    let nonce = uuid::Uuid::new_v4().to_string();
    let key = HostKeyMaterial::load_or_create(
        Some(&stage.join("central-directory/host-ed25519.pk8")),
        HostKeyPolicy::CreateOnly,
        &nonce,
    )?;
    let mut tx = connection.begin().await?;
    sqlx::query("INSERT INTO runtime_host_initialization(singleton,nonce) VALUES(1,?)")
        .bind(&nonce)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO runtime_host_identity(singleton,server_id,public_key,session_hmac_key_fingerprint) VALUES(1,?,?,?)")
        .bind(&server_id).bind(key.public_key().as_slice()).bind(key.session_hmac_key_fingerprint().as_slice()).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn rebuild_and_validate(
    store: &crate::SqliteStore,
    manifest: &BackupManifest,
) -> Result<(), PersistenceError> {
    // Derived current-schema data is rebuilt only after all upgrades succeed.
    let mut tx = store.pool.begin().await?;
    let mut cursor = 0_i64;
    while let Some(row) =
        sqlx::query("SELECT rowid,event_json FROM room_events WHERE rowid>? ORDER BY rowid LIMIT 1")
            .bind(cursor)
            .fetch_optional(&mut *tx)
            .await?
    {
        cursor = row.get("rowid");
        let event =
            crate::backup_json::event(serde_json::from_str(row.get::<&str, _>("event_json"))?)?;
        crate::message_search_index::index_room_message(&mut tx, &event).await?;
    }
    if crate::archive_schema::version(&mut tx).await?
        != crate::schema_version::CURRENT_SCHEMA_VERSION
        || counts(&mut tx).await? != (manifest.rooms, manifest.messages, manifest.assets)
    {
        return Err(invalid());
    }
    crate::backup_database::require_integrity(&mut tx).await?;
    tx.commit().await?;
    store.local_bootstrap_status().await?;
    store.host_identity().await?;
    Ok(())
}

pub(crate) async fn counts(
    connection: &mut SqliteConnection,
) -> Result<(u64, u64, u64), PersistenceError> {
    let rooms: i64 = sqlx::query_scalar("SELECT count(*) FROM rooms")
        .fetch_one(&mut *connection)
        .await?;
    let messages: i64 = sqlx::query_scalar("SELECT count(*) FROM room_events")
        .fetch_one(&mut *connection)
        .await?;
    let mut assets = 0_i64;
    for table in [
        "room_message_attachments",
        "profile_avatar_assets",
        "agent_avatar_assets",
        "connector_avatar_assets",
        "room_appearance_assets",
        "persona_assets",
    ] {
        // Archives from before v99 have no external AI pictures.
        if table == "connector_avatar_assets"
            && !sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?)",
            )
            .bind(table)
            .fetch_one(&mut *connection)
            .await?
        {
            continue;
        }
        assets += sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {table}"
        )))
        .fetch_one(&mut *connection)
        .await?;
    }
    Ok((
        u64::try_from(rooms).map_err(|_| invalid())?,
        u64::try_from(messages).map_err(|_| invalid())?,
        u64::try_from(assets).map_err(|_| invalid())?,
    ))
}

fn require_decode_space(database_bytes: u64, available: u64) -> std::io::Result<()> {
    let required = database_bytes
        .saturating_mul(6)
        .saturating_add(64 * 1024 * 1024);
    if available < required {
        return Err(std::io::ErrorKind::StorageFull.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn capacity_includes_reconstruction_and_sqlite_headroom() {
        let database = 1024 * 1024 * 1024;
        for available in [0, database, database * 2, database * 3] {
            assert!(matches!(require_decode_space(database, available),
                Err(error) if error.kind() == std::io::ErrorKind::StorageFull));
        }
        assert!(require_decode_space(database, database * 16).is_ok());
        assert!(require_decode_space(u64::MAX, 0).is_err());
    }
}

#[cfg(test)]
#[path = "restore_upgrade_tests.rs"]
mod upgrade_tests;
