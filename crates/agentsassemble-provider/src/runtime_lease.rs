#[cfg(unix)]
use std::sync::{Mutex, MutexGuard};
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
#[cfg(unix)]
const CLEANUP_RECEIPT_LOCK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LeaseObservation {
    Active,
    GenerationGone {
        launch_token: String,
    },
    PreviousBoot {
        boot_identity: String,
        launch_token: String,
    },
    Missing,
    Unknown,
}

pub(crate) struct HeldRuntimeLease {
    path: PathBuf,
    #[cfg(unix)]
    lifetime_path: PathBuf,
    #[cfg(unix)]
    launch_lifetime: Mutex<Option<File>>,
    #[cfg(unix)]
    boot_identity: String,
    token: String,
    file: Option<File>,
}

impl HeldRuntimeLease {
    pub(crate) fn prepare(room_id: &str, session_id: &str) -> io::Result<Self> {
        #[cfg(unix)]
        let boot_identity = crate::runtime_boot::current_identity()?.to_owned();
        let path = runtime_lease_path(room_id, session_id)?;
        let mut file = open_runtime_lease(&path)?;
        file.try_lock_exclusive()?;
        #[cfg(unix)]
        let lifetime_path = runtime_lifetime_path(&path);
        #[cfg(unix)]
        let mut lifetime = open_runtime_lease(&lifetime_path)?;
        #[cfg(unix)]
        lifetime.try_lock_exclusive()?;
        let token = Uuid::new_v4().to_string();
        #[cfg(unix)]
        write_marker(&mut lifetime, &format!("lifetime:{token}"))?;
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
            FileExt::unlock(&lifetime)?;
            Ok(Self {
                path,
                lifetime_path,
                launch_lifetime: Mutex::new(None),
                boot_identity,
                token,
                file: None,
            })
        }
    }

    pub(crate) fn new_runtime_handle_id(&self) -> String {
        #[cfg(unix)]
        return crate::runtime_handle::new_unix_handle_id(&self.boot_identity, &self.token);
        #[cfg(not(unix))]
        format!("runtime-v5-windows-{}-{}", self.token, Uuid::new_v4())
    }

    #[cfg(unix)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn token(&self) -> &str {
        &self.token
    }

    pub(crate) fn begin_launch_effect(&self) -> io::Result<()> {
        #[cfg(unix)]
        {
            let mut lifetime = OpenOptions::new()
                .read(true)
                .write(true)
                .open(&self.lifetime_path)?;
            lifetime.try_lock_shared()?;
            if read_marker(&mut lifetime)? != format!("lifetime:{}", self.token) {
                return Err(io::Error::other(
                    "provider launch lifetime generation changed",
                ));
            }
            let mut file = OpenOptions::new().read(true).write(true).open(&self.path)?;
            file.try_lock_exclusive()?;
            if read_marker(&mut file)? != format!("pending:{}", self.token) {
                return Err(io::Error::other("provider launch lease generation changed"));
            }
            write_marker(&mut file, &format!("launching:{}", self.token))?;
            let mut launch_lifetime = lock(&self.launch_lifetime);
            if launch_lifetime.is_some() {
                return Err(io::Error::other(
                    "provider launch lifetime is already owned",
                ));
            }
            *launch_lifetime = Some(lifetime);
        }
        #[cfg(not(unix))]
        if self.file.is_none() {
            return Err(io::Error::other("provider launch lease is unavailable"));
        }
        Ok(())
    }

    #[cfg(unix)]
    pub(crate) fn release_launch_lifetime(&self) {
        lock(&self.launch_lifetime).take();
    }

    pub(crate) fn cleanup_receipt_is_present(&self) -> bool {
        #[cfg(unix)]
        return matches!(
            unix_cleanup_receipt_is_present(&self.path, &self.token),
            Ok(true)
        );
        #[cfg(not(unix))]
        no_cleanup_receipt(self)
    }

    pub(crate) fn cleanup_pre_effect(mut self) {
        #[cfg(unix)]
        self.release_launch_lifetime();
        self.file.take();
        self.remove_files();
    }

    pub(crate) fn release_and_remove(&mut self) {
        #[cfg(unix)]
        self.release_launch_lifetime();
        self.file.take();
        self.remove_files();
    }

    fn remove_files(&self) {
        remove_runtime_lease(&self.path, &self.token);
        #[cfg(unix)]
        remove_runtime_lease(&self.lifetime_path, &self.token);
    }
}

#[cfg(not(unix))]
const fn no_cleanup_receipt(_lease: &HeldRuntimeLease) -> bool {
    false
}

#[cfg(unix)]
pub(crate) fn open_provider_lifetime_lease(path: &Path, token: &str) -> io::Result<File> {
    validate_token(token)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(runtime_lifetime_path(path))?;
    file.try_lock_shared()?;
    if read_marker(&mut file)? != format!("lifetime:{token}") {
        return Err(io::Error::other(
            "provider lifetime lease generation changed",
        ));
    }
    Ok(file)
}

#[cfg(unix)]
pub(crate) fn provider_lifetime_is_active(path: &Path, token: &str) -> io::Result<bool> {
    validate_token(token)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(runtime_lifetime_path(path))?;
    if read_marker(&mut file)? != format!("lifetime:{token}") {
        return Err(io::Error::other(
            "provider lifetime lease generation changed",
        ));
    }
    match file.try_lock_exclusive() {
        Ok(()) => {
            FileExt::unlock(&file)?;
            Ok(false)
        }
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(true),
        Err(error) => Err(error),
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[derive(Clone, Copy)]
pub(crate) struct ProviderLifetimeIdentity {
    pub(crate) device: u64,
    pub(crate) inode: u64,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
pub(crate) fn provider_lifetime_identity(
    path: &Path,
    token: &str,
) -> io::Result<ProviderLifetimeIdentity> {
    use std::os::unix::fs::MetadataExt;

    validate_token(token)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(runtime_lifetime_path(path))?;
    if read_marker(&mut file)? != format!("lifetime:{token}") {
        return Err(io::Error::other(
            "provider lifetime lease generation changed",
        ));
    }
    let metadata = file.metadata()?;
    Ok(ProviderLifetimeIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
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
    if marker != format!("launching:{token}") {
        return Err(io::Error::other(
            "provider runtime lease generation changed",
        ));
    }
    write_marker(
        &mut file,
        &format!(
            "unix:{token}:{}:{}",
            process_group.as_raw_pid(),
            crate::runtime_boot::current_identity()?
        ),
    )?;
    Ok(file)
}

pub(crate) fn observe_runtime_lease(room_id: &str, session_id: &str) -> LeaseObservation {
    let Ok(path) = runtime_lease_path(room_id, session_id) else {
        return LeaseObservation::Unknown;
    };
    let mut file = match OpenOptions::new().read(true).write(true).open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return LeaseObservation::Missing;
        }
        Err(_) => return LeaseObservation::Unknown,
    };
    match file.try_lock_exclusive() {
        Ok(()) => classify_unlocked_marker(&path, read_marker(&mut file)),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => LeaseObservation::Active,
        Err(_) => LeaseObservation::Unknown,
    }
}

fn classify_unlocked_marker(path: &Path, marker: io::Result<String>) -> LeaseObservation {
    let Ok(marker) = marker else {
        return LeaseObservation::Unknown;
    };
    let mut parts = marker.split(':');
    match (
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
        parts.next(),
    ) {
        (Some("pending" | "launching"), Some(token), None, None, None)
            if validate_token(token).is_ok() =>
        {
            classify_unlocked_launch_marker(path, token)
        }
        (Some("windows" | "gone"), Some(token), None, None, None)
            if validate_token(token).is_ok() =>
        {
            LeaseObservation::GenerationGone {
                launch_token: token.to_owned(),
            }
        }
        (Some("unix"), Some(token), Some(raw_pid), Some(boot_identity), None)
            if validate_token(token).is_ok() && is_valid_runtime_boot_identity(boot_identity) =>
        {
            classify_unlocked_unix_marker(path, token, raw_pid, boot_identity)
        }
        _ => LeaseObservation::Unknown,
    }
}

#[cfg(unix)]
fn is_valid_runtime_boot_identity(identity: &str) -> bool {
    crate::runtime_handle::is_valid_boot_identity(identity)
}

#[cfg(not(unix))]
const fn is_valid_runtime_boot_identity(_identity: &str) -> bool {
    false
}

#[cfg(unix)]
fn classify_unlocked_launch_marker(path: &Path, token: &str) -> LeaseObservation {
    let lifetime = provider_lifetime_is_active(path, token);
    if matches!(lifetime, Ok(true)) {
        return LeaseObservation::Active;
    }
    match (
        lifetime,
        crate::unix_process_tree::tagged_runtime_exists(token),
    ) {
        (_, Ok(true)) => LeaseObservation::Active,
        (Ok(false), Ok(false)) => LeaseObservation::GenerationGone {
            launch_token: token.to_owned(),
        },
        _ => LeaseObservation::Unknown,
    }
}

#[cfg(not(unix))]
fn classify_unlocked_launch_marker(_path: &Path, _token: &str) -> LeaseObservation {
    LeaseObservation::Unknown
}

#[cfg(unix)]
fn classify_unlocked_unix_marker(
    path: &Path,
    token: &str,
    raw_pid: &str,
    boot_identity: &str,
) -> LeaseObservation {
    let Ok(current_boot_identity) = crate::runtime_boot::current_identity() else {
        return LeaseObservation::Unknown;
    };
    if boot_identity != current_boot_identity {
        return LeaseObservation::PreviousBoot {
            boot_identity: boot_identity.to_owned(),
            launch_token: token.to_owned(),
        };
    }
    let Some(pid) = raw_pid
        .parse::<i32>()
        .ok()
        .and_then(rustix::process::Pid::from_raw)
    else {
        return LeaseObservation::Unknown;
    };
    match rustix::process::test_kill_process_group(pid) {
        Err(rustix::io::Errno::SRCH) => {
            if matches!(provider_lifetime_is_active(path, token), Ok(true)) {
                return LeaseObservation::Active;
            }
            if matches!(
                crate::unix_process_tree::tagged_runtime_exists(token),
                Ok(true)
            ) {
                return LeaseObservation::Active;
            }
            // Absence or uncertainty of the auxiliary signals is not an
            // absence proof: a normal daemon spawn can close inherited
            // descriptors and clear its environment. Only the guardian
            // may write the exact `gone` receipt after bounded cleanup.
            LeaseObservation::Unknown
        }
        Ok(()) | Err(rustix::io::Errno::PERM) => LeaseObservation::Active,
        Err(_) => LeaseObservation::Unknown,
    }
}

#[cfg(unix)]
pub(crate) fn mark_unix_runtime_gone(path: &Path, token: &str) -> io::Result<()> {
    validate_token(token)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let started = std::time::Instant::now();
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => break,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                if started.elapsed() >= CLEANUP_RECEIPT_LOCK_TIMEOUT {
                    return Err(io::Error::other(
                        "provider runtime cleanup receipt lock timed out",
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(error) => return Err(error),
        }
    }
    let marker = read_marker(&mut file)?;
    if marker_token(&marker) != Some(token) || !marker.starts_with("unix:") {
        return Err(io::Error::other(
            "provider runtime lease generation changed before cleanup receipt",
        ));
    }
    write_marker(&mut file, &format!("gone:{token}"))
}

#[cfg(unix)]
pub(crate) fn unix_cleanup_receipt_is_present(path: &Path, token: &str) -> io::Result<bool> {
    validate_token(token)?;
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    file.try_lock_exclusive()?;
    Ok(read_marker(&mut file)? == format!("gone:{token}"))
}

#[cfg(not(unix))]
fn classify_unlocked_unix_marker(
    _path: &Path,
    _token: &str,
    _raw_pid: &str,
    _boot_identity: &str,
) -> LeaseObservation {
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
        (Some("pending" | "launching" | "windows" | "unix" | "lifetime" | "gone"), Some(token))
            if validate_token(token).is_ok() =>
        {
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

#[cfg(unix)]
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    let parsed = Uuid::parse_str(token)
        .map_err(|_| io::Error::other("provider runtime lease token is invalid"))?;
    if parsed.to_string() != token {
        return Err(io::Error::other("provider runtime lease token is invalid"));
    }
    Ok(())
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

#[cfg(unix)]
fn runtime_lifetime_path(lease_path: &Path) -> PathBuf {
    lease_path.with_extension("lifetime")
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

#[cfg(test)]
pub(crate) fn cleanup_stale_runtime_lease(room_id: &str, session_id: &str) {
    if let Ok(lease) = HeldRuntimeLease::prepare(room_id, session_id) {
        lease.cleanup_pre_effect();
    }
}

#[cfg(all(test, unix))]
pub(crate) fn lock_test_launch_lifetime(room_id: &str, session_id: &str) -> File {
    let path = runtime_lease_path(room_id, session_id)
        .unwrap_or_else(|error| panic!("resolve test runtime lease: {error}"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(runtime_lifetime_path(&path))
        .unwrap_or_else(|error| panic!("open test launch lifetime: {error}"));
    file.try_lock_exclusive()
        .unwrap_or_else(|error| panic!("lock test launch lifetime: {error}"));
    file
}

#[cfg(all(test, unix))]
mod tests {
    use super::{HeldRuntimeLease, LeaseObservation, observe_runtime_lease};

    fn previous_boot_handle(current: &str) -> String {
        let mut bytes = current.as_bytes().to_vec();
        let first_boot_digit = "runtime-v5-".len();
        bytes[first_boot_digit] = if bytes[first_boot_digit] == b'0' {
            b'1'
        } else {
            b'0'
        };
        String::from_utf8(bytes).unwrap_or_else(|error| panic!("encode prior boot handle: {error}"))
    }

    #[test]
    fn exact_pending_and_missing_leases_are_distinct_gone_proofs() {
        let lease = HeldRuntimeLease::prepare("lease-test-room", "lease-test-session")
            .unwrap_or_else(|error| panic!("prepare runtime lease: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-test-room", "lease-test-session"),
            LeaseObservation::GenerationGone {
                launch_token: lease.token().to_owned()
            }
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
            super::classify_unlocked_marker(
                std::path::Path::new("unused"),
                Ok("not-a-runtime-marker".to_owned()),
            ),
            LeaseObservation::Unknown
        );
    }

    #[test]
    fn launching_lease_requires_the_handoff_lifetime_or_runtime_evidence() {
        let lease = HeldRuntimeLease::prepare("lease-launch-room", "lease-launch-session")
            .unwrap_or_else(|error| panic!("prepare launch lease: {error}"));
        lease
            .begin_launch_effect()
            .unwrap_or_else(|error| panic!("begin launch effect: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-launch-room", "lease-launch-session"),
            LeaseObservation::Active
        );
        lease.release_launch_lifetime();
        assert_eq!(
            observe_runtime_lease("lease-launch-room", "lease-launch-session"),
            LeaseObservation::GenerationGone {
                launch_token: lease.token().to_owned()
            }
        );
        lease.cleanup_pre_effect();
    }

    #[test]
    fn unlocked_unix_lease_requires_recorded_group_absence() {
        let lease = HeldRuntimeLease::prepare("lease-group-room", "lease-group-session")
            .unwrap_or_else(|error| panic!("prepare group runtime lease: {error}"));
        lease
            .begin_launch_effect()
            .unwrap_or_else(|error| panic!("begin group runtime launch: {error}"));
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

    #[test]
    fn unlocked_unix_lease_requires_guardian_cleanup_receipt() {
        let lease = HeldRuntimeLease::prepare("lease-lifetime-room", "lease-lifetime-session")
            .unwrap_or_else(|error| panic!("prepare lifetime runtime lease: {error}"));
        lease
            .begin_launch_effect()
            .unwrap_or_else(|error| panic!("begin lifetime runtime launch: {error}"));
        lease.release_launch_lifetime();
        let lifetime = super::open_provider_lifetime_lease(lease.path(), lease.token())
            .unwrap_or_else(|error| panic!("open provider lifetime lease: {error}"));
        let absent_group = rustix::process::Pid::from_raw(i32::MAX)
            .unwrap_or_else(|| panic!("construct absent process group"));
        let anchor = super::activate_unix_runtime_lease(lease.path(), lease.token(), absent_group)
            .unwrap_or_else(|error| panic!("activate lifetime runtime lease: {error}"));
        drop(anchor);
        assert_eq!(
            observe_runtime_lease("lease-lifetime-room", "lease-lifetime-session"),
            LeaseObservation::Active
        );
        drop(lifetime);
        assert_eq!(
            observe_runtime_lease("lease-lifetime-room", "lease-lifetime-session"),
            LeaseObservation::Unknown
        );
        super::mark_unix_runtime_gone(lease.path(), lease.token())
            .unwrap_or_else(|error| panic!("record guardian cleanup receipt: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-lifetime-room", "lease-lifetime-session"),
            LeaseObservation::GenerationGone {
                launch_token: lease.token().to_owned()
            }
        );
        lease.cleanup_pre_effect();
    }

    #[test]
    fn previous_boot_unix_marker_is_a_process_absence_proof() {
        use fs2::FileExt;

        let lease = HeldRuntimeLease::prepare("lease-old-boot-room", "lease-old-boot-session")
            .unwrap_or_else(|error| panic!("prepare old-boot runtime lease: {error}"));
        lease
            .begin_launch_effect()
            .unwrap_or_else(|error| panic!("begin old-boot runtime launch: {error}"));
        lease.release_launch_lifetime();
        let current_handle = lease.new_runtime_handle_id();
        let old_handle = previous_boot_handle(&current_handle);
        let old_boot = crate::runtime_handle::parse_handle_id(&old_handle)
            .unwrap_or_else(|error| panic!("decode old-boot runtime handle: {error}"))
            .boot_identity
            .unwrap_or_else(|| panic!("Unix runtime handle must bind boot identity"));
        let mut file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(lease.path())
            .unwrap_or_else(|error| panic!("open old-boot runtime lease: {error}"));
        file.try_lock_exclusive()
            .unwrap_or_else(|error| panic!("lock old-boot runtime lease: {error}"));
        super::write_marker(
            &mut file,
            &format!("unix:{}:{}:{old_boot}", lease.token(), i32::MAX),
        )
        .unwrap_or_else(|error| panic!("write old-boot runtime marker: {error}"));
        FileExt::unlock(&file)
            .unwrap_or_else(|error| panic!("unlock old-boot runtime lease: {error}"));
        assert_eq!(
            observe_runtime_lease("lease-old-boot-room", "lease-old-boot-session"),
            LeaseObservation::PreviousBoot {
                boot_identity: old_boot,
                launch_token: lease.token().to_owned()
            }
        );
        lease.cleanup_pre_effect();
    }
}
