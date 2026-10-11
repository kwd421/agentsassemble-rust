//! Private, secret-free export progress and exact restart cleanup ownership.
use super::{BackupControl, Job, Progress};
use anyhow::{Context, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};
use tokio_util::sync::CancellationToken;

#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    staging: Option<PathBuf>,
    partial: Option<PathBuf>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    progress: Progress,
    record: Record,
}

pub(super) fn save(job: &Job) -> anyhow::Result<()> {
    let mut file = tempfile::Builder::new()
        .prefix(".backup-journal-")
        .tempfile_in(&job.root)?;
    serde_json::to_writer(
        &mut file,
        &serde_json::json!({"progress":job.progress,"record":job.record}),
    )?;
    file.flush()?;
    file.as_file().sync_all()?;
    file.persist(job.root.join("server-backup.json"))?;
    #[cfg(unix)]
    std::fs::File::open(&job.root)?.sync_all()?;
    Ok(())
}

impl BackupControl {
    pub(crate) fn recover(root: &Path) -> anyhow::Result<Self> {
        let path = root.join("server-backup.json");
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::default());
            }
            Err(error) => return Err(error.into()),
            Ok(meta) => ensure!(
                meta.is_file() && !meta.file_type().is_symlink() && meta.len() <= 16384,
                "invalid backup journal"
            ),
        }
        let Journal {
            mut progress,
            record,
        } = serde_json::from_slice(&std::fs::read(path)?)?;
        cleanup(root, progress.operation_id, &record)?;
        if !matches!(
            progress.phase.as_str(),
            "complete" | "failed" | "cancelled" | "interrupted"
        ) {
            // Publication may have completed before its journal write. Never infer success,
            // remove the destination, or silently retry with a lost passphrase.
            progress.phase = "interrupted".into();
        }
        let job = Job {
            progress,
            record: Record::default(),
            root: root.to_owned(),
            cancel: CancellationToken::new(),
        };
        save(&job)?;
        Ok(Self(std::sync::Arc::new(parking_lot::Mutex::new(Some(
            job,
        )))))
    }

    pub(super) fn record_path(
        &self,
        id: uuid::Uuid,
        path: &Path,
        partial: bool,
    ) -> anyhow::Result<()> {
        let mut current = self.0.lock();
        let job = current
            .as_mut()
            .filter(|job| job.progress.operation_id == id)
            .context("backup job changed")?;
        if partial {
            job.record.partial = Some(path.into());
        } else {
            job.record.staging = Some(path.into());
        }
        save(job)
    }
}

fn remove_file(path: &Path) -> anyhow::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn cleanup(root: &Path, id: uuid::Uuid, record: &Record) -> anyhow::Result<()> {
    if let Some(path) = &record.partial {
        ensure!(
            path.is_absolute()
                && path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n
                    .starts_with(&format!(".aa-backup-{id}-"))
                    && n.ends_with(".partial")),
            "invalid backup partial ownership"
        );
        remove_file(path)?;
    }
    let Some(stage) = &record.staging else {
        return Ok(());
    };
    ensure!(
        stage.parent() == Some(root)
            && stage
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!("server-backup-{id}-"))),
        "invalid backup stage ownership"
    );
    match std::fs::symlink_metadata(stage) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
        Ok(meta) => ensure!(
            meta.is_dir() && !meta.file_type().is_symlink(),
            "invalid backup stage"
        ),
    }
    for name in [
        "snapshot.sqlite",
        "payload.sqlite",
        "payload.sqlite-journal",
        "payload.sqlite-wal",
        "payload.sqlite-shm",
    ] {
        remove_file(&stage.join(name))?;
    }
    std::fs::remove_dir(stage)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restart_cleans_only_recorded_scratch_and_keeps_publication_uncertain() -> anyhow::Result<()>
    {
        let root = tempfile::tempdir()?;
        let output = tempfile::tempdir()?;
        let id = uuid::Uuid::new_v4();
        let stage = tempfile::Builder::new()
            .prefix(&format!("server-backup-{id}-"))
            .tempdir_in(root.path())?
            .keep();
        std::fs::write(stage.join("snapshot.sqlite"), b"private snapshot")?;
        std::fs::write(stage.join("payload.sqlite"), b"private payload")?;
        let partial = tempfile::Builder::new()
            .prefix(&format!(".aa-backup-{id}-"))
            .suffix(".partial")
            .tempfile_in(output.path())?;
        let (_, partial) = partial.keep()?;
        let destination = output.path().join("completed.aaserver");
        std::fs::write(&destination, b"possibly published backup")?;
        let unrelated = root.path().join("user-data");
        std::fs::write(&unrelated, b"preserve")?;
        save(&Job {
            progress: Progress {
                operation_id: id,
                phase: "publishing".into(),
                snapshot_at: None,
                bytes: None,
            },
            cancel: CancellationToken::new(),
            root: root.path().into(),
            record: Record {
                staging: Some(stage.clone()),
                partial: Some(partial.clone()),
            },
        })?;
        let recovered = BackupControl::recover(root.path())?;
        assert_eq!(
            recovered
                .0
                .lock()
                .as_ref()
                .context("missing job")?
                .progress
                .phase,
            "interrupted"
        );
        assert!(!stage.exists());
        assert!(!partial.exists());
        assert_eq!(std::fs::read(destination)?, b"possibly published backup");
        assert_eq!(std::fs::read(unrelated)?, b"preserve");
        let again = BackupControl::recover(root.path())?;
        assert_eq!(
            again
                .0
                .lock()
                .as_ref()
                .context("missing job")?
                .progress
                .operation_id,
            id
        );
        Ok(())
    }
}
