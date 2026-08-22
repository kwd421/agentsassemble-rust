use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Read, Seek, Write},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use sha2::{Digest, Sha256};
use uuid::Uuid;

const MAX_LEASE_MARKER_BYTES: u64 = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LeaseObservation {
    Active,
    Gone,
    Missing,
    Unknown,
}

pub(crate) struct HeldRuntimeLease {
    path: PathBuf,
    token: String,
    file: Option<File>,
}

impl HeldRuntimeLease {
    pub(crate) fn prepare(room_id: &str, session_id: &str) -> io::Result<Self> {
        let path = runtime_lease_path(room_id, session_id)?;
        let mut file = open_runtime_lease(&path)?;
        file.try_lock_exclusive()?;
        let token = Uuid::new_v4().to_string();
        write_marker(&mut file, &format!("pending:{token}"))?;
        #[cfg(windows)]
        {
            write_marker(&mut file, &format!("windows:{token}"))?;
            Ok(Self {
                path,
                token,
                file: Some(file),
            })
        }
        #[cfg(not(windows))]
        {
            FileExt::unlock(&file)?;
            Ok(Self {
                path,
                token,
                file: None,
            })
        }
    }

    #[cfg(unix)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn cleanup_pre_effect(mut self) {
        self.file.take();
        remove_runtime_lease(&self.path, &self.token);
    }

    pub(crate) fn release_and_remove(&mut self) {
        self.file.take();
        remove_runtime_lease(&self.path, &self.token);
    }
}

#[cfg(unix)]
pub(crate) fn activate_unix_runtime_lease(
    path: &Path,
    token: &str,
    process_group: rustix::process::Pid,
) -> io::Result<File> {
    validate_token(token)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.try_lock_exclusive()?;
    let marker = read_marker(&mut file)?;
    if marker != format!("pending:{token}") {
        return Err(io::Error::other(
            "provider runtime lease generation changed",
        ));
    }
    write_marker(
        &mut file,
        &format!("unix:{token}:{}", process_group.as_raw_pid()),
    )?;
    Ok(file)
}

pub(crate) fn observe_runtime_lease(room_id: &str, session_id: &str) -> LeaseObservation {
    let Ok(path) = runtime_lease_path(room_id, session_id) else {
        return LeaseObservation::Unknown;
    };
    let mut file = match OpenOptions::new().read(true).write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return LeaseObservation::Missing;
        }
        Err(_) => return LeaseObservation::Unknown,
    };
    match file.try_lock_exclusive() {
        Ok(()) => classify_unlocked_marker(read_marker(&mut file)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => LeaseObservation::Active,
        Err(_) => LeaseObservation::Unknown,
    }
}

fn classify_unlocked_marker(marker: io::Result<String>) -> LeaseObservation {
    let Ok(marker) = marker else {
        return LeaseObservation::Unknown;
    };
    let mut parts = marker.split(':');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some("pending" | "windows"), Some(token), None, None) if validate_token(token).is_ok() => {
            LeaseObservation::Gone
        }
        (Some("unix"), Some(token), Some(raw_pid), None) if validate_token(token).is_ok() => {
            classify_unlocked_unix_marker(raw_pid)
        }
        _ => LeaseObservation::Unknown,
    }
}

#[cfg(unix)]
fn classify_unlocked_unix_marker(raw_pid: &str) -> LeaseObservation {
    let Some(pid) = raw_pid
        .parse::<i32>()
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return LeaseObservation::Unknown;
    };
    match rustix::process::test_kill_process_group(pid) {
        Err(rustix::io::Errno::SRCH) => LeaseObservation::Gone,
        Ok(()) | Err(rustix::io::Errno::PERM) => LeaseObservation::Active,
        Err(_) => LeaseObservation::Unknown,
    }
}

#[cfg(not(unix))]
fn classify_unlocked_unix_marker(_raw_pid: &str) -> LeaseObservation {
    LeaseObservation::Unknown
}

fn remove_runtime_lease(path: &Path, token: &str) {
    let Ok(mut file) = OpenOptions::new().read(true).write(true).open(path) else {
        return;
    };
    if file.try_lock_exclusive().is_err() {
        return;
    }
    let Ok(marker) = read_marker(&mut file) else {
        return;
    };
    if marker_token(&marker) != Some(token) {
        return;
    }
    let _ = std::fs::remove_file(path);
}

fn marker_token(marker: &str) -> Option<&str> {
    let mut parts = marker.split(':');
    match (parts.next(), parts.next()) {
        (Some("pending" | "windows" | "unix"), Some(token)) if validate_token(token).is_ok() => {
            Some(token)
        }
        _ => None,
    }
}

fn open_runtime_lease(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    options.open(path)
}

fn read_marker(file: &mut File) -> io::Result<String> {
    let metadata = file.metadata()?;
    if metadata.len() == 0 || metadata.len() > MAX_LEASE_MARKER_BYTES {
        return Err(io::Error::other("provider runtime lease marker is invalid"));
    }
    file.rewind()?;
    let mut marker = String::new();
    file.take(MAX_LEASE_MARKER_BYTES + 1)
        .read_to_string(&mut marker)?;
    if marker.len() as u64 > MAX_LEASE_MARKER_BYTES {
        return Err(io::Error::other(
            "provider runtime lease marker is oversized",
        ));
    }
    Ok(marker.trim_end_matches('\n').to_owned())
}

fn write_marker(file: &mut File, marker: &str) -> io::Result<()> {
    if marker.is_empty() || marker.len() as u64 > MAX_LEASE_MARKER_BYTES {
        return Err(io::Error::other("provider runtime lease marker is invalid"));
    }
    file.set_len(0)?;
    file.rewind()?;
    file.write_all(marker.as_bytes())?;
    file.write_all(b"\n")?;
    file.sync_all()
}

fn validate_token(token: &str) -> io::Result<()> {
    Uuid::parse_str(token)
        .map(|_| ())
        .map_err(|_| io::Error::other("provider runtime lease token is invalid"))
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
pub(crate) fn cleanup_stale_runtime_lease(room_id: &str, session_id: &str) {
    if let Ok(lease) = HeldRuntimeLease::prepare(room_id, session_id) {
        lease.cleanup_pre_effect();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::{HeldRuntimeLease, LeaseObservation, observe_runtime_lease};

    #[test]
    fn exact_pending_and_missing_leases_are_distinct_gone_proofs() {
        let lease = HeldRuntimeLease::prepare("lease-test-room", "lease-test-session")
            .unwrap_or_else(|error| panic!("prepare runtime lease: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-test-room", "lease-test-session"),
            LeaseObservation::Gone
        );
        let mut lease = lease;
        lease.release_and_remove();
        assert_eq!(
            observe_runtime_lease("lease-test-room", "lease-test-session"),
            LeaseObservation::Missing
        );
    }

    #[test]
    fn malformed_unlocked_lease_never_proves_absence() {
        assert_eq!(
            super::classify_unlocked_marker(Ok("not-a-runtime-marker".to_owned())),
            LeaseObservation::Unknown
        );
    }

    #[test]
    fn unlocked_unix_lease_requires_recorded_group_absence() {
        let lease = HeldRuntimeLease::prepare("lease-group-room", "lease-group-session")
            .unwrap_or_else(|error| panic!("prepare group runtime lease: {error}"));
        let anchor = super::activate_unix_runtime_lease(
            lease.path(),
            lease.token(),
            rustix::process::getpgrp(),
        )
        .unwrap_or_else(|error| panic!("activate group runtime lease: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-group-room", "lease-group-session"),
            LeaseObservation::Active
        );
        drop(anchor);
        assert_eq!(
            observe_runtime_lease("lease-group-room", "lease-group-session"),
            LeaseObservation::Active
        );
        lease.cleanup_pre_effect();
    }
}
