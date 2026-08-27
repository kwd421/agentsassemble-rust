use std::{
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
};

use agentsassemble_persistence::secure_private_directory;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

const LOCK_TIMEOUT: Duration = Duration::from_secs(95);
const LOCK_RETRY: Duration = Duration::from_millis(50);
const MAX_OWNER_RECORD_BYTES: u64 = 128;

#[derive(Clone)]
pub(super) struct StableOwnership {
    owner_id: Arc<str>,
    directory: Arc<Path>,
}

#[derive(Clone, Copy)]
pub(super) enum OwnershipFailure {
    Cancelled,
    Unavailable,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StableOwnerRecord {
    owner_id: String,
}

impl StableOwnership {
    pub(super) fn new(state_root: &Path) -> Self {
        Self {
            owner_id: Uuid::new_v4().to_string().into(),
            directory: Arc::from(state_root.join("runtime").join("stable-entry")),
        }
    }

    pub(super) async fn claim(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<(), OwnershipFailure> {
        let ownership = self.clone();
        let cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || {
            let _guard = ownership.acquire_lock(&cancellation)?;
            ownership.read_owner()?;
            ownership.write_owner()
        })
        .await
        .map_err(|_| OwnershipFailure::Unavailable)?
    }

    pub(super) async fn lock_if_current(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Option<File>, OwnershipFailure> {
        let ownership = self.clone();
        let cancellation = cancellation.clone();
        tokio::task::spawn_blocking(move || {
            let guard = ownership.acquire_lock(&cancellation)?;
            let current = ownership
                .read_owner()?
                .ok_or(OwnershipFailure::Unavailable)?;
            if current == ownership.owner_id.as_ref() {
                Ok(Some(guard))
            } else {
                Ok(None)
            }
        })
        .await
        .map_err(|_| OwnershipFailure::Unavailable)?
    }

    fn acquire_lock(&self, cancellation: &CancellationToken) -> Result<File, OwnershipFailure> {
        prepare_ownership_directory(&self.directory).map_err(|_| OwnershipFailure::Unavailable)?;
        let path = self.directory.join("owner.lock");
        reject_non_regular_entry(&path).map_err(|_| OwnershipFailure::Unavailable)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map_err(|_| OwnershipFailure::Unavailable)?;
        if !file
            .metadata()
            .map_err(|_| OwnershipFailure::Unavailable)?
            .is_file()
        {
            return Err(OwnershipFailure::Unavailable);
        }
        let started = Instant::now();
        loop {
            if cancellation.is_cancelled() {
                return Err(OwnershipFailure::Cancelled);
            }
            match FileExt::try_lock_exclusive(&file) {
                Ok(()) => return Ok(file),
                Err(error)
                    if error.kind() == io::ErrorKind::WouldBlock
                        && started.elapsed() < LOCK_TIMEOUT =>
                {
                    std::thread::sleep(LOCK_RETRY);
                }
                Err(_) => return Err(OwnershipFailure::Unavailable),
            }
        }
    }

    fn read_owner(&self) -> Result<Option<String>, OwnershipFailure> {
        let path = self.directory.join("owner.json");
        reject_non_regular_entry(&path).map_err(|_| OwnershipFailure::Unavailable)?;
        let file = match File::open(path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(OwnershipFailure::Unavailable),
        };
        if !file
            .metadata()
            .map_err(|_| OwnershipFailure::Unavailable)?
            .is_file()
        {
            return Err(OwnershipFailure::Unavailable);
        }
        let mut bytes = Vec::new();
        file.take(MAX_OWNER_RECORD_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| OwnershipFailure::Unavailable)?;
        if bytes.len() as u64 > MAX_OWNER_RECORD_BYTES {
            return Err(OwnershipFailure::Unavailable);
        }
        let record: StableOwnerRecord =
            serde_json::from_slice(&bytes).map_err(|_| OwnershipFailure::Unavailable)?;
        let owner_id = Uuid::parse_str(&record.owner_id)
            .map_err(|_| OwnershipFailure::Unavailable)?
            .to_string();
        if owner_id != record.owner_id {
            return Err(OwnershipFailure::Unavailable);
        }
        Ok(Some(owner_id))
    }

    fn write_owner(&self) -> Result<(), OwnershipFailure> {
        let path = self.directory.join("owner.json");
        reject_non_regular_entry(&path).map_err(|_| OwnershipFailure::Unavailable)?;
        let mut file = tempfile::NamedTempFile::new_in(self.directory.as_ref())
            .map_err(|_| OwnershipFailure::Unavailable)?;
        serde_json::to_writer(
            &mut file,
            &StableOwnerRecord {
                owner_id: self.owner_id.to_string(),
            },
        )
        .map_err(|_| OwnershipFailure::Unavailable)?;
        file.write_all(b"\n")
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| OwnershipFailure::Unavailable)?;
        file.persist(path)
            .map(|_| ())
            .map_err(|_| OwnershipFailure::Unavailable)
    }
}

fn prepare_ownership_directory(path: &Path) -> io::Result<()> {
    let Some(runtime) = path.parent() else {
        return Err(io::Error::other("stable-entry state has no runtime parent"));
    };
    ensure_private_directory(runtime)?;
    ensure_private_directory(path)
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match std::fs::create_dir(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(io::Error::other(
            "stable-entry state path is not a directory",
        ));
    }
    secure_private_directory(path)
}

fn reject_non_regular_entry(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            io::Error::other("stable-entry state path is not a regular file"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
