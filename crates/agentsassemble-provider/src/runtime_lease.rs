use std::{
    env,
    fs::{File, OpenOptions},
    io,
    path::PathBuf,
};

use fs2::FileExt;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseObservation {
    Active,
    Gone,
    Unknown,
}

pub(crate) struct HeldRuntimeLease {
    _file: File,
}

impl HeldRuntimeLease {
    #[cfg(windows)]
    pub(crate) fn acquire_for_parent(room_id: &str, session_id: &str) -> io::Result<Option<Self>> {
        let file = replace_stale_lease(room_id, session_id)?;
        file.try_lock_exclusive()?;
        Ok(Some(Self { _file: file }))
    }

    #[cfg(not(windows))]
    #[expect(
        clippy::unnecessary_wraps,
        reason = "the cross-platform caller preserves Windows lease acquisition failure"
    )]
    pub(crate) fn acquire_for_parent(room_id: &str, session_id: &str) -> io::Result<Option<Self>> {
        let _ = (room_id, session_id);
        Ok(None)
    }
}

#[cfg(unix)]
pub(crate) fn prepare_unheld_runtime_lease(room_id: &str, session_id: &str) -> io::Result<PathBuf> {
    let path = runtime_lease_path(room_id, session_id)?;
    drop(replace_stale_lease(room_id, session_id)?);
    Ok(path)
}

pub(crate) fn observe_runtime_lease(room_id: &str, session_id: &str) -> LeaseObservation {
    let Ok(path) = runtime_lease_path(room_id, session_id) else {
        return LeaseObservation::Unknown;
    };
    let Ok(file) = OpenOptions::new().read(true).write(true).open(path) else {
        return LeaseObservation::Unknown;
    };
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            LeaseObservation::Gone
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => LeaseObservation::Active,
        Err(_) => LeaseObservation::Unknown,
    }
}

pub(crate) fn remove_runtime_lease(room_id: &str, session_id: &str) {
    let Ok(path) = runtime_lease_path(room_id, session_id) else {
        return;
    };
    let _ = std::fs::remove_file(path);
}

fn replace_stale_lease(room_id: &str, session_id: &str) -> io::Result<File> {
    let path = runtime_lease_path(room_id, session_id)?;
    match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(stale) => {
            stale.try_lock_exclusive()?;
            std::fs::remove_file(&path)?;
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    let file = options.open(path)?;
    file.sync_all()?;
    Ok(file)
}

fn runtime_lease_path(room_id: &str, session_id: &str) -> io::Result<PathBuf> {
    if room_id.is_empty() || session_id.is_empty() {
        return Err(io::Error::other("runtime lease identity is empty"));
    }
    let mut digest = Sha256::new();
    for value in [room_id, session_id] {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    }
    let encoded = format!("{:x}", digest.finalize());
    Ok(runtime_lease_root()?.join(format!("{encoded}.lease")))
}

fn runtime_lease_root() -> io::Result<PathBuf> {
    #[cfg(unix)]
    let suffix = rustix::process::geteuid().as_raw().to_string();
    #[cfg(not(unix))]
    let suffix = "local".to_owned();
    let root = env::temp_dir().join(format!("agentsassemble-provider-runtime-{suffix}"));
    match std::fs::create_dir(&root) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
            }
        }
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = std::fs::symlink_metadata(&root)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("runtime lease directory is invalid"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};

        if metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(io::Error::other("runtime lease directory is not private"));
        }
    }
    Ok(root)
}

#[cfg(all(test, unix))]
mod tests {
    use super::{LeaseObservation, observe_runtime_lease, prepare_unheld_runtime_lease};

    #[test]
    fn unlocked_exact_lease_proves_a_runtime_is_gone() {
        let path = prepare_unheld_runtime_lease("lease-test-room", "lease-test-session")
            .unwrap_or_else(|error| panic!("prepare runtime lease: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-test-room", "lease-test-session"),
            LeaseObservation::Gone
        );
        std::fs::remove_file(path)
            .unwrap_or_else(|error| panic!("remove runtime lease fixture: {error}"));
    }
}
