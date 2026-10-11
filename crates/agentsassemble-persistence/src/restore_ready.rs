//! The running writer acknowledges a restore only after central accepts its
//! current online endpoint. A pre-runtime caller cannot declare the host ready.
use crate::{
    CentralEndpointEvent, PersistenceError, SqliteStore,
    backup_json::invalid,
    restore_job::{RestoreJob, RestorePhase, marker_path, save_journal},
};

impl SqliteStore {
    /// Called by the authenticated endpoint publication owner after its exact ACK.
    /// # Errors
    /// Rejects mismatched runtime identity/epoch or unexpected staging contents.
    pub async fn acknowledge_restored_endpoint(
        &self,
        event: &CentralEndpointEvent,
        fingerprint: &str,
    ) -> Result<(), PersistenceError> {
        let Some(target) = &self.database_path else {
            return Ok(());
        };
        let Some(mut journal) = RestoreJob::inspect(target)? else {
            return Ok(());
        };
        if !event.online() || self.current_central_endpoint_generation().await? != event.generation
        {
            return Ok(());
        }
        if !matches!(journal.phase, RestorePhase::Activated | RestorePhase::Ready)
            || journal.new_key_fingerprint != fingerprint
            || journal.committed_epoch != event.registration_epoch
            || self.registration_epoch().await?.as_deref() != Some(&event.registration_epoch)
            || event.generation <= journal.endpoint_generation.ok_or_else(invalid)?
            || journal.manifest.as_ref().ok_or_else(invalid)?.server_id
                != self.host_identity().await?.server_id()
        {
            return Err(invalid());
        }
        journal.phase = RestorePhase::Ready;
        save_journal(target, &journal)?;
        let stage = target
            .parent()
            .ok_or_else(invalid)?
            .join(format!(".server-move-{}", journal.operation_id));
        match std::fs::symlink_metadata(&stage) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return finish_cleanup(target);
            }
            Err(error) => return Err(PersistenceError::WriterLease(error)),
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(invalid());
            }
            Ok(_) => (),
        }
        // Exact files owned by this journal, never recursive directory removal.
        for name in [
            "decoded.sqlite",
            "decoded.sqlite-wal",
            "decoded.sqlite-shm",
            "decoded.sqlite-journal",
            "runtime.sqlite3.writer.lock",
            "activation-link-probe",
        ] {
            match std::fs::remove_file(stage.join(name)) {
                Ok(()) => (),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
                Err(error) => return Err(PersistenceError::WriterLease(error)),
            }
        }
        match std::fs::remove_dir(stage.join("central-directory")) {
            Ok(()) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(PersistenceError::WriterLease(error)),
        }
        std::fs::remove_dir(stage).map_err(PersistenceError::WriterLease)?;
        finish_cleanup(target)
    }
}

fn finish_cleanup(target: &std::path::Path) -> Result<(), PersistenceError> {
    // Ready is durable before cleanup. Remove the recovery fence last, including
    // older Ready installations and crashes after staging was already removed.
    std::fs::remove_file(marker_path(target)).map_err(PersistenceError::WriterLease)?;
    #[cfg(unix)]
    crate::restore_job::sync_directory(target.parent().ok_or_else(invalid)?)?;
    Ok(())
}
