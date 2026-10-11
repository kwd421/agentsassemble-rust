//! Durable switch intent and no-clobber activation. A committed central move can
//! only roll forward with the same key and epoch; unknown results are not cancelled.
use crate::{
    PersistenceError,
    backup_json::invalid,
    restore_job::{RestoreJob, RestorePhase},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{io::Read, path::Path};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreReceipt {
    pub operation_id: String,
    pub expected_epoch: String,
    pub new_key_fingerprint: String,
    pub registration_epoch: String,
    pub status: String,
    pub endpoint_generation: i64,
}

impl RestoreJob {
    /// Bind the current incarnation confirmed by the owner before fresh step-up.
    /// # Errors
    /// Refuses rebinding an existing operation to another key or expected epoch.
    pub fn bind_switch(
        &mut self,
        expected_epoch: &str,
        new_key_fingerprint: &str,
    ) -> Result<(), PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::Prepared | RestorePhase::SwitchUnknown
        ) || expected_epoch.is_empty()
            || expected_epoch.len() > 128
            || new_key_fingerprint.len() != 43
        {
            return Err(invalid());
        }
        if (!self.journal.expected_epoch.is_empty()
            && self.journal.expected_epoch != expected_epoch)
            || (!self.journal.new_key_fingerprint.is_empty()
                && self.journal.new_key_fingerprint != new_key_fingerprint)
        {
            return Err(invalid());
        }
        self.journal.expected_epoch = expected_epoch.into();
        self.journal.new_key_fingerprint = new_key_fingerprint.into();
        self.save()
    }

    /// Must commit before sending the central mutation, including its first byte.
    /// # Errors
    /// Refuses an unprepared or unbound operation.
    pub fn mark_switch_unknown(&mut self) -> Result<(), PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::Prepared | RestorePhase::SwitchUnknown
        ) || self.journal.expected_epoch.is_empty()
            || self.journal.new_key_fingerprint.is_empty()
        {
            return Err(invalid());
        }
        self.journal.phase = RestorePhase::SwitchUnknown;
        self.save()
    }

    /// A single authenticated central read proved both no receipt and a changed
    /// incarnation, so no in-flight old-epoch request can still commit.
    /// # Errors
    /// Rejects a result for another operation or expected epoch.
    pub fn record_not_committed(
        &mut self,
        operation: &str,
        expected_epoch: &str,
    ) -> Result<(), PersistenceError> {
        if self.journal.phase != RestorePhase::SwitchUnknown
            || operation != self.journal.operation_id.to_string()
            || expected_epoch != self.journal.expected_epoch
        {
            return Err(invalid());
        }
        self.journal.phase = RestorePhase::NotCommitted;
        self.save()
    }

    /// Consume an authenticated response for this exact operation and key.
    /// The native central transport owns origin/session/response authentication.
    /// # Errors
    /// Rejects unknown, superseded, borrowed or mismatched receipts.
    pub fn record_commit(&mut self, receipt: &RestoreReceipt) -> Result<(), PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::SwitchUnknown | RestorePhase::Committed
        ) || receipt.status != "committed"
            || !(1..=9_007_199_254_740_991).contains(&receipt.endpoint_generation)
            || self
                .journal
                .endpoint_generation
                .is_some_and(|floor| floor != receipt.endpoint_generation)
            || receipt.operation_id != self.journal.operation_id.to_string()
            || receipt.expected_epoch != self.journal.expected_epoch
            || receipt.new_key_fingerprint != self.journal.new_key_fingerprint
            || receipt.registration_epoch.is_empty()
            || receipt.registration_epoch.len() > 128
            || receipt.registration_epoch == receipt.expected_epoch
            || (!self.journal.committed_epoch.is_empty()
                && self.journal.committed_epoch != receipt.registration_epoch)
        {
            return Err(invalid());
        }
        self.journal
            .committed_epoch
            .clone_from(&receipt.registration_epoch);
        self.journal.endpoint_generation = Some(receipt.endpoint_generation);
        self.journal.phase = RestorePhase::Committed;
        self.save()
    }

    /// Install the committed generation without replacing any unrelated file.
    /// Native UI must still start the runtime and verify endpoint publication.
    /// # Errors
    /// Leaves a durable resumable marker on I/O failure; never rolls central back.
    pub async fn activate(&mut self) -> Result<(), PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::Committed | RestorePhase::Activating
        ) {
            return Err(invalid());
        }
        let database = self.stage.join("runtime.sqlite3");
        let key = self.stage.join("central-directory/host-ed25519.pk8");
        if self.journal.phase == RestorePhase::Committed {
            let mut connection =
                SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&database))
                    .await?;
            let changed=sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES('central_registration_epoch',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value WHERE runtime_metadata.value=excluded.value")
                .bind(&self.journal.committed_epoch).execute(&mut connection).await?;
            if changed.rows_affected() != 1 {
                return Err(invalid());
            }
            sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES('central_endpoint_generation',?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
                .bind(self.journal.endpoint_generation.ok_or_else(invalid)?.to_string())
                .execute(&mut connection).await?;
            connection.close().await?;
            self.journal.database_hash = Some(file_hash(&database)?);
            self.journal.key_hash = Some(file_hash(&key)?);
            self.journal.phase = RestorePhase::Activating;
            self.save()?;
        }
        let parent = self.target.parent().ok_or_else(invalid)?;
        let key_directory = parent.join("central-directory");
        if !key_directory.exists() {
            std::fs::create_dir(&key_directory).map_err(PersistenceError::WriterLease)?;
        }
        let metadata =
            std::fs::symlink_metadata(&key_directory).map_err(PersistenceError::WriterLease)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(invalid());
        }
        crate::private_fs::secure_private_directory(&key_directory)
            .map_err(PersistenceError::WriterLease)?;
        activate_file(
            &key,
            &key_directory.join("host-ed25519.pk8"),
            self.journal.key_hash.ok_or_else(invalid)?,
        )?;
        activate_file(
            &database,
            &self.target,
            self.journal.database_hash.ok_or_else(invalid)?,
        )?;
        #[cfg(unix)]
        for directory in [
            parent,
            &key_directory,
            &self.stage,
            &self.stage.join("central-directory"),
        ] {
            crate::restore_job::sync_directory(directory)?;
        }
        self.journal.phase = RestorePhase::Activated;
        self.save()
    }
}

fn file_hash(path: &Path) -> Result<[u8; 32], PersistenceError> {
    let metadata = std::fs::symlink_metadata(path).map_err(PersistenceError::WriterLease)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid());
    }
    let mut file = std::fs::File::open(path).map_err(PersistenceError::WriterLease)?;
    let mut hash = Sha256::new();
    let mut bytes = zeroize::Zeroizing::new(vec![0_u8; 64 * 1024]);
    loop {
        let n = file
            .read(&mut bytes)
            .map_err(PersistenceError::WriterLease)?;
        if n == 0 {
            break;
        }
        hash.update(&bytes[..n]);
    }
    Ok(hash.finalize().into())
}

fn activate_file(source: &Path, target: &Path, expected: [u8; 32]) -> Result<(), PersistenceError> {
    match std::fs::symlink_metadata(target) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if file_hash(source)? != expected {
                return Err(invalid());
            }
            // Atomic no-replace publication on the same local volume. Unsupported
            // filesystems fail explicitly; no copy/overwrite fallback exists.
            std::fs::hard_link(source, target).map_err(PersistenceError::WriterLease)?;
        }
        Err(error) => return Err(PersistenceError::WriterLease(error)),
        Ok(_) => {}
    }
    if file_hash(target)? != expected {
        return Err(invalid());
    }
    match std::fs::symlink_metadata(source) {
        Ok(_) => {
            if file_hash(source)? != expected {
                return Err(invalid());
            }
            std::fs::remove_file(source).map_err(PersistenceError::WriterLease)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(PersistenceError::WriterLease(error)),
    }
    Ok(())
}
