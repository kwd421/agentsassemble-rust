use std::{io, path::Path, time::Duration};

use rustix::process::{Pid, Signal};

pub(crate) const RUNTIME_TOKEN_ENV: &str = "AGENTSASSEMBLE_INTERNAL_RUNTIME_TOKEN";

const MAX_SCANNED_PROCESSES: usize = 65_536;
const MAX_CAPTURED_PROCESSES: usize = 512;
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAX_ENVIRONMENT_BYTES: u64 = 64 * 1024;
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAX_SCANNED_DESCRIPTORS: usize = 1_048_576;
#[cfg(target_os = "linux")]
const MAX_CAPTURE_PASSES: usize = 8;

pub(crate) fn tagged_runtime_exists(token: &str) -> io::Result<bool> {
    Ok(!tagged_processes(token)?.is_empty())
}

pub(crate) struct CapturedRuntimeProcesses {
    #[cfg(target_os = "linux")]
    escaped: Vec<rustix::fd::OwnedFd>,
    captured_count: usize,
}

impl CapturedRuntimeProcesses {
    pub(crate) fn freeze(lease_path: &Path, token: &str, anchored_group: Pid) -> io::Result<Self> {
        rustix::process::kill_process_group(anchored_group, Signal::STOP)
            .map_err(|error| io::Error::other(format!("stop anchor group: {error}")))?;
        capture_escaped(lease_path, token, anchored_group)
            .map_err(|error| io::Error::other(format!("capture escaped runtime: {error}")))
    }

    pub(crate) fn kill(&self) -> io::Result<()> {
        if self.captured_count > MAX_CAPTURED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process capture budget was exceeded",
            ));
        }
        #[cfg(target_os = "linux")]
        for process in self.escaped.iter().rev() {
            match rustix::process::pidfd_send_signal(process, Signal::KILL) {
                Ok(()) | Err(rustix::io::Errno::SRCH) => {}
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    pub(crate) fn confirm_gone(
        &self,
        lease_path: &Path,
        token: &str,
        deadline: Duration,
    ) -> io::Result<()> {
        if self.captured_count > MAX_CAPTURED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process capture budget was exceeded",
            ));
        }
        let started = std::time::Instant::now();
        while tagged_runtime_exists(token)?
            || crate::runtime_lease::provider_lifetime_is_active(lease_path, token)?
        {
            if started.elapsed() >= deadline {
                return Err(io::Error::other(
                    "provider runtime processes remained after shutdown",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn capture_escaped(
    lease_path: &Path,
    token: &str,
    anchored_group: Pid,
) -> io::Result<CapturedRuntimeProcesses> {
    use std::collections::HashSet;

    use rustix::process::{PidfdFlags, pidfd_open, pidfd_send_signal};

    let lifetime = crate::runtime_lease::provider_lifetime_identity(lease_path, token)?;
    let mut escaped = Vec::new();
    let mut captured = HashSet::new();
    for _ in 0..MAX_CAPTURE_PASSES {
        let mut added = false;
        let mut unstable = false;
        for identity in owned_processes(token, lifetime)? {
            let Some(process) = Pid::from_raw(identity.pid) else {
                continue;
            };
            if rustix::process::getpgid(Some(process)) == Ok(anchored_group)
                || captured.contains(&identity)
            {
                continue;
            }
            if escaped.len() >= MAX_CAPTURED_PROCESSES {
                return Err(io::Error::other(
                    "provider runtime process capture budget was exceeded",
                ));
            }
            let process_fd = match pidfd_open(process, PidfdFlags::empty()) {
                Ok(process_fd) => process_fd,
                Err(rustix::io::Errno::SRCH) => {
                    unstable = true;
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            if !owned_identity_matches(identity, token, lifetime)? {
                unstable = true;
                continue;
            }
            match rustix::process::getpgid(Some(process)) {
                Ok(group) if group == anchored_group => continue,
                Ok(_) => {}
                Err(rustix::io::Errno::SRCH) => {
                    unstable = true;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            if !owned_identity_matches(identity, token, lifetime)? {
                unstable = true;
                continue;
            }
            match pidfd_send_signal(&process_fd, Signal::STOP) {
                Ok(()) => {}
                Err(rustix::io::Errno::SRCH) => {
                    unstable = true;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            if !owned_identity_matches(identity, token, lifetime)? {
                return Err(io::Error::other(
                    "provider runtime identity changed while it was captured",
                ));
            }
            captured.insert(identity);
            escaped.push(process_fd);
            added = true;
        }
        if !added && !unstable {
            let captured_count = escaped.len();
            return Ok(CapturedRuntimeProcesses {
                escaped,
                captured_count,
            });
        }
    }
    Err(io::Error::other(
        "provider runtime process capture did not stabilize",
    ))
}

#[cfg(target_os = "android")]
fn capture_escaped(
    lease_path: &Path,
    token: &str,
    anchored_group: Pid,
) -> io::Result<CapturedRuntimeProcesses> {
    refuse_unstable_escaped_processes(lease_path, token, anchored_group, true)?;
    Ok(CapturedRuntimeProcesses { captured_count: 0 })
}

#[cfg(target_os = "macos")]
fn capture_escaped(
    lease_path: &Path,
    token: &str,
    anchored_group: Pid,
) -> io::Result<CapturedRuntimeProcesses> {
    refuse_unstable_escaped_processes(lease_path, token, anchored_group, false)?;
    Ok(CapturedRuntimeProcesses { captured_count: 0 })
}

#[cfg(any(target_os = "android", target_os = "macos"))]
fn refuse_unstable_escaped_processes(
    lease_path: &Path,
    token: &str,
    anchored_group: Pid,
    include_descendants: bool,
) -> io::Result<()> {
    #[cfg(target_os = "macos")]
    let _ = lease_path;
    let processes = if include_descendants {
        #[cfg(target_os = "android")]
        {
            let lifetime = crate::runtime_lease::provider_lifetime_identity(lease_path, token)?;
            owned_processes(token, lifetime)?
                .into_iter()
                .filter_map(|identity| Pid::from_raw(identity.pid))
                .collect()
        }
        #[cfg(not(target_os = "android"))]
        unreachable!()
    } else {
        tagged_processes(token)?
    };
    for process in processes {
        match rustix::process::getpgid(Some(process)) {
            Ok(group) if group == anchored_group => {}
            Err(rustix::io::Errno::SRCH) => {}
            Ok(_) => {
                return Err(io::Error::other(
                    "an escaped provider process cannot be terminated by stable identity",
                ));
            }
            Err(error) => {
                return Err(io::Error::other(format!(
                    "inspect tagged provider process group: {error}"
                )));
            }
        }
    }
    Ok(())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ProcessIdentity {
    pid: i32,
    start_time: u64,
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn owned_processes(
    token: &str,
    lifetime: crate::runtime_lease::ProviderLifetimeIdentity,
) -> io::Result<Vec<ProcessIdentity>> {
    use std::collections::{HashMap, HashSet};

    let snapshot = process_snapshot()?;
    let mut owned = HashMap::<i32, ProcessIdentity>::new();
    let mut scanned_descriptors = 0_usize;
    for identity in snapshot.values().copied() {
        let Some(process) = Pid::from_raw(identity.pid) else {
            continue;
        };
        if process_has_lifetime(process, lifetime, &mut scanned_descriptors)? {
            owned.insert(identity.pid, identity);
        }
    }
    let mut seen = owned.keys().copied().collect::<HashSet<_>>();
    for process in tagged_processes(token)? {
        if seen.insert(process.as_raw_pid()) {
            let Some(identity) = process_identity(process)? else {
                continue;
            };
            owned.insert(identity.pid, identity);
        }
    }
    Ok(owned.into_values().collect())
}

#[cfg(target_os = "linux")]
fn owned_identity_matches(
    expected: ProcessIdentity,
    token: &str,
    lifetime: crate::runtime_lease::ProviderLifetimeIdentity,
) -> io::Result<bool> {
    let Some(process) = Pid::from_raw(expected.pid) else {
        return Ok(false);
    };
    if process_identity(process)? != Some(expected) {
        return Ok(false);
    }
    let mut scanned_descriptors = 0;
    Ok(
        process_has_lifetime(process, lifetime, &mut scanned_descriptors)?
            || process_has_token(process, token)?,
    )
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_has_lifetime(
    process: Pid,
    lifetime: crate::runtime_lease::ProviderLifetimeIdentity,
    scanned: &mut usize,
) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt;

    let directory = format!("/proc/{}/fd", process.as_raw_pid());
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        *scanned = scanned.saturating_add(1);
        if *scanned > MAX_SCANNED_DESCRIPTORS {
            return Err(io::Error::other(
                "provider runtime descriptor scan budget was exceeded",
            ));
        }
        let metadata = match std::fs::metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.dev() == lifetime.device && metadata.ino() == lifetime.inode {
            return Ok(true);
        }
    }
    Ok(false)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_snapshot() -> io::Result<std::collections::HashMap<i32, ProcessIdentity>> {
    use std::os::unix::fs::MetadataExt;

    let mut snapshot = std::collections::HashMap::new();
    let mut scanned = 0_usize;
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(raw_pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<i32>().ok())
        else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        if scanned > MAX_SCANNED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process scan budget was exceeded",
            ));
        }
        let metadata = match std::fs::metadata(entry.path()) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.uid() != rustix::process::geteuid().as_raw() {
            continue;
        }
        let Some(pid) = Pid::from_raw(raw_pid) else {
            continue;
        };
        if let Some(identity) = process_identity(pid)? {
            snapshot.insert(raw_pid, identity);
        }
    }
    Ok(snapshot)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_identity(process: Pid) -> io::Result<Option<ProcessIdentity>> {
    let path = format!("/proc/{}/stat", process.as_raw_pid());
    let contents = match std::fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let fields = contents
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("provider process stat is invalid"))?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let start_time = fields
        .get(19)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("provider process start time is invalid"))?;
    Ok(Some(ProcessIdentity {
        pid: process.as_raw_pid(),
        start_time,
    }))
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn capture_escaped(
    _lease_path: &Path,
    _token: &str,
    _anchored_group: Pid,
) -> io::Result<CapturedRuntimeProcesses> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "provider runtime process capture is unsupported",
    ))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn tagged_processes(token: &str) -> io::Result<Vec<Pid>> {
    use std::os::unix::fs::MetadataExt;

    let mut matches = Vec::new();
    let mut scanned = 0_usize;
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Some(raw_pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse::<i32>().ok())
        else {
            continue;
        };
        scanned = scanned.saturating_add(1);
        if scanned > MAX_SCANNED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process scan budget was exceeded",
            ));
        }
        let process_path = entry.path();
        let metadata = match std::fs::metadata(&process_path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(error),
        };
        if metadata.uid() != rustix::process::geteuid().as_raw() {
            continue;
        }
        let Some(pid) = Pid::from_raw(raw_pid) else {
            continue;
        };
        if process_has_token(pid, token)? {
            matches.push(pid);
            if matches.len() > MAX_CAPTURED_PROCESSES {
                return Err(io::Error::other(
                    "provider runtime process scan match budget was exceeded",
                ));
            }
        }
    }
    Ok(matches)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_has_token(pid: Pid, token: &str) -> io::Result<bool> {
    use std::io::Read;

    let path = format!("/proc/{}/environ", pid.as_raw_pid());
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut environment = Vec::new();
    file.take(MAX_ENVIRONMENT_BYTES + 1)
        .read_to_end(&mut environment)?;
    if environment.len() as u64 > MAX_ENVIRONMENT_BYTES {
        return Err(io::Error::other(
            "provider runtime process environment was oversized",
        ));
    }
    let expected = format!("{RUNTIME_TOKEN_ENV}={token}");
    Ok(environment
        .split(|byte| *byte == 0)
        .any(|entry| entry == expected.as_bytes()))
}

#[cfg(target_os = "macos")]
fn tagged_processes(token: &str) -> io::Result<Vec<Pid>> {
    use std::os::unix::ffi::OsStrExt;

    use sysinfo::{ProcessRefreshKind, ProcessStatus, ProcessesToUpdate, System, UpdateKind};

    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing()
            .with_user(UpdateKind::Always)
            .with_environ(UpdateKind::Always),
    );
    if system.processes().len() > MAX_SCANNED_PROCESSES {
        return Err(io::Error::other(
            "provider runtime process scan budget was exceeded",
        ));
    }
    let expected = format!("{RUNTIME_TOKEN_ENV}={token}");
    let effective_user = rustix::process::geteuid().as_raw();
    let mut matches = Vec::new();
    for (raw_pid, process) in system.processes() {
        if process
            .user_id()
            .is_none_or(|user_id| **user_id != effective_user)
        {
            continue;
        }
        if matches!(
            process.status(),
            ProcessStatus::Zombie | ProcessStatus::Dead
        ) {
            continue;
        }
        if !process
            .environ()
            .iter()
            .any(|entry| entry.as_os_str().as_bytes() == expected.as_bytes())
        {
            continue;
        }
        let pid = i32::try_from(raw_pid.as_u32())
            .ok()
            .and_then(Pid::from_raw)
            .ok_or_else(|| io::Error::other("provider runtime process id is invalid"))?;
        matches.push(pid);
        if matches.len() > MAX_CAPTURED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process scan match budget was exceeded",
            ));
        }
    }
    Ok(matches)
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn tagged_processes(_token: &str) -> io::Result<Vec<Pid>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "provider runtime process scan is unsupported",
    ))
}
