//! Closed diagnostic vocabulary: never format errors, paths, SQL, or payload data.
use super::BackupControl;
use agentsassemble_persistence::{PersistenceError, backup_codec::BackupError};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum FailureReason {
    PermissionDenied,
    DestinationExists,
    StorageFull,
    PathUnavailable,
    Io,
    DatabaseBusy,
    Database,
    InvalidData,
    Encryption,
    Verification,
    Internal,
}

impl FailureReason {
    fn classify(error: &anyhow::Error) -> Self {
        // Prefer inner I/O causes over their codec/persistence wrappers.
        for cause in error.chain() {
            if let Some(io) = cause.downcast_ref::<std::io::Error>() {
                return match io.kind() {
                    std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
                    std::io::ErrorKind::AlreadyExists => Self::DestinationExists,
                    std::io::ErrorKind::StorageFull => Self::StorageFull,
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory => {
                        Self::PathUnavailable
                    }
                    _ => Self::Io,
                };
            }
        }
        for cause in error.chain() {
            if let Some(persistence) = cause.downcast_ref::<PersistenceError>() {
                return match persistence {
                    PersistenceError::Database(error)
                        if error.as_database_error().is_some_and(|db| {
                            matches!(
                                db.code().as_deref(),
                                Some("5" | "6" | "261" | "262" | "517")
                            )
                        }) =>
                    {
                        Self::DatabaseBusy
                    }
                    PersistenceError::Database(_) => Self::Database,
                    PersistenceError::InvalidBackup => Self::InvalidData,
                    _ => Self::Internal,
                };
            }
            if let Some(codec) = cause.downcast_ref::<BackupError>() {
                return match codec {
                    BackupError::Crypto => Self::Encryption,
                    BackupError::Io(_) => Self::Io,
                    _ => Self::Verification,
                };
            }
        }
        Self::Internal
    }
}

impl BackupControl {
    pub(super) fn report_failure(&self, id: uuid::Uuid, error: &anyhow::Error) {
        if let Some(job) = self
            .0
            .lock()
            .as_mut()
            .filter(|job| job.progress.operation_id == id)
        {
            let reason = FailureReason::classify(error);
            tracing::error!(operation_id = %id, phase = %job.progress.phase, reason = ?reason, "server backup export failed");
            job.progress.failure_reason = Some(reason);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn diagnostics_are_closed_codes_even_when_causes_contain_secrets() -> anyhow::Result<()> {
        let error = anyhow::Error::new(BackupError::Io(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "secret passphrase /private/path",
        )));
        let reason = FailureReason::classify(&error);
        assert_eq!(reason, FailureReason::PermissionDenied);
        assert_eq!(serde_json::to_string(&reason)?, "\"permission_denied\"");
        assert_eq!(
            FailureReason::classify(&anyhow::anyhow!("private payload")),
            FailureReason::Internal
        );
        Ok(())
    }
}
