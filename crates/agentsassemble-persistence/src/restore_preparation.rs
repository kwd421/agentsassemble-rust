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
    /// Validate the complete payload and provision a fresh host/session identity.
    /// # Errors
    /// Refuses changed bytes, inconsistent manifest/data, or any existing stage DB.
    pub async fn prepare(
        &mut self,
        input: &Path,
        manifest: BackupManifest,
    ) -> Result<(), PersistenceError> {
        if self.journal.phase != RestorePhase::Validating || self.journal.manifest.is_some() {
            return Err(invalid());
        }
        let mut input_file = std::fs::File::open(input).map_err(PersistenceError::WriterLease)?;
        let (size, hash) =
            crate::backup_codec::measure_payload(&mut input_file).map_err(|_| invalid())?;
        if size != manifest.database_bytes || hash != manifest.database_sha256 {
            return Err(invalid());
        }
        let free = fs2::available_space(&self.stage).map_err(PersistenceError::WriterLease)?;
        if free < size.saturating_mul(2) {
            return Err(PersistenceError::WriterLease(std::io::Error::from(
                std::io::ErrorKind::StorageFull,
            )));
        }
        let database = self.stage.join("runtime.sqlite3");
        crate::backup_database::reconstruct_payload(input, &database).await?;
        let mut connection =
            SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database))
                .await?;
        let server_id: String =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='server_id'")
                .fetch_one(&mut connection)
                .await?;
        let schema: String =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
                .fetch_one(&mut connection)
                .await?;
        let counts = counts(&mut connection).await?;
        if server_id != manifest.server_id
            || schema != manifest.schema_version.to_string()
            || counts != (manifest.rooms, manifest.messages, manifest.assets)
        {
            return Err(invalid());
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        let key = HostKeyMaterial::load_or_create(
            Some(&self.stage.join("central-directory/host-ed25519.pk8")),
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
        // The destination schema is owned DDL; its FTS triggers rebuild from the
        // validated canonical history, never from supplied virtual-table pages.
        let mut cursor = 0_i64;
        while let Some(row) = sqlx::query(
            "SELECT rowid,event_json FROM room_events WHERE rowid>? ORDER BY rowid LIMIT 1",
        )
        .bind(cursor)
        .fetch_optional(&mut *tx)
        .await?
        {
            cursor = row.get("rowid");
            let event =
                crate::backup_json::event(serde_json::from_str(row.get::<&str, _>("event_json"))?)?;
            crate::message_search_index::index_room_message(&mut tx, &event).await?;
        }
        tx.commit().await?;
        connection.close().await?;
        // Validate the ordinary persistence owner's key binding and bootstrap digest
        // before allowing any central request. All execution queues were sanitized.
        let store = crate::SqliteStore::open_path(&database).await?;
        store.local_bootstrap_status().await?;
        store.host_identity().await?;
        store.close().await?;
        self.preflight_activation()?;
        #[cfg(unix)]
        for directory in [&self.stage, &self.stage.join("central-directory")] {
            crate::restore_job::sync_directory(directory)?;
        }
        self.journal.manifest = Some(manifest);
        self.journal.phase = RestorePhase::Prepared;
        self.save()
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
        let store = crate::SqliteStore::open_path(&database).await?;
        let identity = store.host_identity().await?;
        store.close().await?;
        Ok(identity)
    }
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
        "room_appearance_assets",
        "persona_assets",
    ] {
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
