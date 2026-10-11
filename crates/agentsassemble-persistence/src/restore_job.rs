//! Pre-runtime ownership of a move. The ordinary runtime and importer share the
//! same writer lease; a durable marker prevents startup between importer calls.
use crate::{PersistenceError, backup_json::invalid};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RestorePhase {
    Validating,
    Cancelling,
    Prepared,
    SwitchUnknown,
    NotCommitted,
    Committed,
    Activating,
    Activated,
    Ready,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreJournal {
    pub operation_id: uuid::Uuid,
    pub phase: RestorePhase,
    pub manifest: Option<crate::backup_codec::BackupManifest>,
    pub new_key_fingerprint: String,
    pub expected_epoch: String,
    pub committed_epoch: String,
    pub endpoint_generation: Option<i64>,
    pub database_hash: Option<[u8; 32]>,
    pub key_hash: Option<[u8; 32]>,
}

pub struct RestoreJob {
    pub(crate) journal: RestoreJournal,
    pub(crate) target: PathBuf,
    pub(crate) stage: PathBuf,
    lease: File,
}

pub(crate) fn marker_path(target: &Path) -> PathBuf {
    let mut path = target.as_os_str().to_owned();
    path.push(".restore.json");
    PathBuf::from(path)
}

pub(crate) fn require_no_import(target: &Path) -> Result<(), PersistenceError> {
    match std::fs::symlink_metadata(marker_path(target)) {
        Ok(_) => {
            let journal = read_journal(target)?;
            if matches!(journal.phase, RestorePhase::Activated | RestorePhase::Ready) {
                Ok(())
            } else {
                Err(PersistenceError::RestorePending)
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

fn absent(path: &Path) -> Result<bool, PersistenceError> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(false),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

fn installation_absent(target: &Path) -> Result<bool, PersistenceError> {
    let parent = target.parent().ok_or_else(invalid)?;
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let mut path = target.as_os_str().to_owned();
        path.push(suffix);
        if !absent(Path::new(&path))? {
            return Ok(false);
        }
    }
    absent(&parent.join("central-directory/host-ed25519.pk8"))
}

fn read_journal(target: &Path) -> Result<RestoreJournal, PersistenceError> {
    let path = marker_path(target);
    let metadata = std::fs::symlink_metadata(&path).map_err(PersistenceError::WriterLease)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 8192 {
        return Err(invalid());
    }
    let mut file = File::open(&path).map_err(PersistenceError::WriterLease)?;
    crate::database_target::validate_link_count(
        &file,
        &file.metadata().map_err(PersistenceError::WriterLease)?,
    )?;
    if !crate::private_fs::validate_file(&file, &path).map_err(PersistenceError::WriterLease)? {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(PersistenceError::WriterLease)?;
    serde_json::from_slice(&bytes).map_err(|_| invalid())
}

impl RestoreJob {
    /// Reserve a truly absent installation. Zero-room and zero-byte ordinary
    /// databases are existing data, never candidates for replacement.
    /// # Errors
    /// Rejects an existing database/key/job or concurrent runtime owner.
    pub fn begin(target: &Path) -> Result<Self, PersistenceError> {
        let parent = target
            .parent()
            .ok_or_else(invalid)?
            .canonicalize()
            .map_err(PersistenceError::WriterLease)?;
        if !crate::private_fs::validate_directory(&parent).map_err(PersistenceError::WriterLease)? {
            return Err(invalid());
        }
        let target = parent.join(target.file_name().ok_or_else(invalid)?);
        let lease = crate::database_target::acquire_writer_lease(&target)?;
        if !installation_absent(&target)? {
            return Err(PersistenceError::InitializationNotAllowed);
        }
        require_no_import(&target)?;
        let operation_id = uuid::Uuid::new_v4();
        let stage = parent.join(format!(".server-move-{operation_id}"));
        std::fs::create_dir(&stage).map_err(PersistenceError::WriterLease)?;
        crate::private_fs::secure_private_directory(&stage)
            .map_err(PersistenceError::WriterLease)?;
        let journal = RestoreJournal {
            operation_id,
            phase: RestorePhase::Validating,
            manifest: None,
            new_key_fingerprint: String::new(),
            expected_epoch: String::new(),
            committed_epoch: String::new(),
            endpoint_generation: None,
            database_hash: None,
            key_hash: None,
        };
        let job = Self {
            journal,
            target,
            stage,
            lease,
        };
        let mut file =
            tempfile::NamedTempFile::new_in(&parent).map_err(PersistenceError::WriterLease)?;
        crate::private_fs::secure_file(file.as_file(), file.path())
            .map_err(PersistenceError::WriterLease)?;
        serde_json::to_writer(file.as_file_mut(), &job.journal)?;
        file.as_file_mut()
            .flush()
            .and_then(|()| file.as_file().sync_all())
            .map_err(PersistenceError::WriterLease)?;
        file.persist_noclobber(marker_path(&job.target))
            .map_err(|error| PersistenceError::WriterLease(error.error))?;
        #[cfg(unix)]
        sync_directory(&parent)?;
        Ok(job)
    }

    /// Resume exactly the durable import job; no unmarked database can enter here.
    /// # Errors
    /// Rejects malformed or borrowed jobs, changed staging and concurrent owners.
    pub fn resume(target: &Path, operation: uuid::Uuid) -> Result<Self, PersistenceError> {
        let parent = target
            .parent()
            .ok_or_else(invalid)?
            .canonicalize()
            .map_err(PersistenceError::WriterLease)?;
        let target = parent.join(target.file_name().ok_or_else(invalid)?);
        let lease = crate::database_target::acquire_writer_lease(&target)?;
        let journal = read_journal(&target)?;
        if journal.operation_id != operation {
            return Err(invalid());
        }
        let stage = parent.join(format!(".server-move-{operation}"));
        if matches!(
            journal.phase,
            RestorePhase::Validating
                | RestorePhase::Prepared
                | RestorePhase::NotCommitted
                | RestorePhase::Cancelling
        ) && absent(&stage)?
            && installation_absent(&target)?
        {
            return Ok(Self {
                journal,
                target,
                stage,
                lease,
            });
        }
        let metadata = std::fs::symlink_metadata(&stage).map_err(PersistenceError::WriterLease)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || !crate::private_fs::validate_directory(&stage)
                .map_err(PersistenceError::WriterLease)?
        {
            return Err(invalid());
        }
        if matches!(
            journal.phase,
            RestorePhase::Validating
                | RestorePhase::Cancelling
                | RestorePhase::Prepared
                | RestorePhase::SwitchUnknown
                | RestorePhase::NotCommitted
                | RestorePhase::Committed
        ) && !installation_absent(&target)?
        {
            return Err(invalid());
        }
        Ok(Self {
            journal,
            target,
            stage,
            lease,
        })
    }

    /// Read existing move progress without creating a database or a writer lease.
    /// # Errors
    /// Rejects unreadable or malformed local journals.
    pub fn inspect(target: &Path) -> Result<Option<RestoreJournal>, PersistenceError> {
        if absent(&marker_path(target))? {
            Ok(None)
        } else {
            read_journal(target).map(Some)
        }
    }

    /// Cancel only before any switch request could have reached central.
    /// # Errors
    /// Rejects uncertain/committed moves and unexpected staging entries.
    pub fn cancel(self) -> Result<(), PersistenceError> {
        if !matches!(
            self.journal.phase,
            RestorePhase::Validating
                | RestorePhase::Prepared
                | RestorePhase::Cancelling
                | RestorePhase::NotCommitted
        ) {
            return Err(invalid());
        }
        if read_journal(&self.target)?.operation_id != self.journal.operation_id {
            return Err(invalid());
        }
        // No new journal allocation: ENOSPC must not strand the payload. All
        // cancellable phases can resume missing/partially removed staging; the
        // existing marker fences ordinary startup until the last deletion.
        for name in [
            "decoded.sqlite",
            "decoded.sqlite-wal",
            "decoded.sqlite-shm",
            "decoded.sqlite-journal",
            "runtime.sqlite3",
            "runtime.sqlite3-wal",
            "runtime.sqlite3-shm",
            "runtime.sqlite3-journal",
            "runtime.sqlite3.writer.lock",
            "activation-link-probe",
            "central-directory/host-ed25519.pk8",
        ] {
            let path = self.stage.join(name);
            if !absent(&path)? {
                std::fs::remove_file(path).map_err(PersistenceError::WriterLease)?;
            }
        }
        let keys = self.stage.join("central-directory");
        if !absent(&keys)? {
            std::fs::remove_dir(keys).map_err(PersistenceError::WriterLease)?;
        }
        if !absent(&self.stage)? {
            std::fs::remove_dir(&self.stage).map_err(PersistenceError::WriterLease)?;
        }
        std::fs::remove_file(marker_path(&self.target)).map_err(PersistenceError::WriterLease)?;
        #[cfg(unix)]
        sync_directory(self.target.parent().ok_or_else(invalid)?)?;
        Ok(())
    }

    #[must_use]
    pub const fn journal(&self) -> &RestoreJournal {
        &self.journal
    }
    #[must_use]
    pub fn stage(&self) -> &Path {
        &self.stage
    }

    pub(crate) fn save(&mut self) -> Result<(), PersistenceError> {
        save_journal(&self.target, &self.journal)
    }
}

pub(crate) fn save_journal(
    target: &Path,
    journal: &RestoreJournal,
) -> Result<(), PersistenceError> {
    let parent = target.parent().ok_or_else(invalid)?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(PersistenceError::WriterLease)?;
    crate::private_fs::secure_file(temporary.as_file(), temporary.path())
        .map_err(PersistenceError::WriterLease)?;
    serde_json::to_writer(temporary.as_file_mut(), journal)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(PersistenceError::WriterLease)?;
    temporary
        .persist(marker_path(target))
        .map_err(|error| PersistenceError::WriterLease(error.error))?;
    #[cfg(unix)]
    sync_directory(parent)?;
    Ok(())
}

#[cfg(unix)]
pub(crate) fn sync_directory(path: &Path) -> Result<(), PersistenceError> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(PersistenceError::WriterLease)
}

impl Drop for RestoreJob {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lease);
    }
}

#[cfg(test)]
#[path = "restore_job_tests.rs"]
mod tests;
