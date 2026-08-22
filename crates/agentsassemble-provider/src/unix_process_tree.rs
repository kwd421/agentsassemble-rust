use std::{io, time::Duration};

use rustix::process::{Pid, Signal};

pub(crate) const RUNTIME_TOKEN_ENV: &str = "AGENTSASSEMBLE_INTERNAL_RUNTIME_TOKEN";

const MAX_SCANNED_PROCESSES: usize = 65_536;
const MAX_CAPTURED_PROCESSES: usize = 512;
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAX_ENVIRONMENT_BYTES: u64 = 64 * 1024;
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
    pub(crate) fn freeze(token: &str, anchored_group: Pid) -> io::Result<Self> {
        rustix::process::kill_process_group(anchored_group, Signal::STOP)
            .map_err(|error| io::Error::other(format!("stop anchor group: {error}")))?;
        capture_escaped(token, anchored_group)
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

    pub(crate) fn confirm_gone(&self, token: &str, deadline: Duration) -> io::Result<()> {
        if self.captured_count > MAX_CAPTURED_PROCESSES {
            return Err(io::Error::other(
                "provider runtime process capture budget was exceeded",
            ));
        }
        let started = std::time::Instant::now();
        while tagged_runtime_exists(token)? {
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
fn capture_escaped(token: &str, anchored_group: Pid) -> io::Result<CapturedRuntimeProcesses> {
    use std::collections::HashSet;

    use rustix::process::{PidfdFlags, pidfd_open, pidfd_send_signal};

    let mut escaped = Vec::new();
    let mut captured_pids = HashSet::new();
    for _ in 0..MAX_CAPTURE_PASSES {
        let mut added = false;
        let mut unstable = false;
        for process in tagged_processes(token)? {
            if rustix::process::getpgid(Some(process)) == Ok(anchored_group)
                || captured_pids.contains(&process.as_raw_pid())
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
            if !process_has_token(process, token)? {
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
            match pidfd_send_signal(&process_fd, Signal::STOP) {
                Ok(()) => {}
                Err(rustix::io::Errno::SRCH) => {
                    unstable = true;
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            captured_pids.insert(process.as_raw_pid());
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
fn capture_escaped(token: &str, anchored_group: Pid) -> io::Result<CapturedRuntimeProcesses> {
    refuse_unstable_escaped_processes(token, anchored_group)?;
    Ok(CapturedRuntimeProcesses { captured_count: 0 })
}

#[cfg(target_os = "macos")]
fn capture_escaped(token: &str, anchored_group: Pid) -> io::Result<CapturedRuntimeProcesses> {
    refuse_unstable_escaped_processes(token, anchored_group)?;
    Ok(CapturedRuntimeProcesses { captured_count: 0 })
}

#[cfg(any(target_os = "android", target_os = "macos"))]
fn refuse_unstable_escaped_processes(token: &str, anchored_group: Pid) -> io::Result<()> {
    for process in tagged_processes(token)? {
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

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn capture_escaped(_token: &str, _anchored_group: Pid) -> io::Result<CapturedRuntimeProcesses> {
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
