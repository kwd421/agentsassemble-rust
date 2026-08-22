use std::{
    env,
    ffi::OsStr,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};

use rustix::process::{Pid, Signal};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const GUARDIAN_FLAG: &str = "--agentsassemble-provider-guardian";
const ANCHOR_FLAG: &str = "--agentsassemble-provider-anchor";
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_ANCHOR=";
const MAX_HELPER_OUTPUT_BYTES: usize = 8 * 1024;
const TEST_MODE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_MODE";
const TEST_LEASE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_LEASE";
const TEST_TOKEN_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_TOKEN";

use crate::filesystem::{BoundExecutable, bind_helper_executable_sync};
use crate::unix_process_tree::CapturedRuntimeProcesses;

#[derive(Clone)]
pub(crate) struct GuardianLaunch {
    executable: Arc<BoundExecutable>,
    test_harness: bool,
}

impl GuardianLaunch {
    pub(crate) fn production(executable: &Path) -> io::Result<Self> {
        Ok(Self {
            executable: Arc::new(bind_helper_executable_sync(executable)?),
            test_harness: false,
        })
    }

    #[cfg(test)]
    pub(crate) fn test_harness() -> io::Result<Self> {
        Ok(Self {
            executable: Arc::new(bind_helper_executable_sync(&reexecution_path()?)?),
            test_harness: true,
        })
    }

    pub(crate) fn guardian_command(
        &self,
        lease_path: &Path,
        lease_token: &str,
    ) -> io::Result<tokio::process::Command> {
        let mut command = tokio::process::Command::new(self.executable.launch_path());
        self.configure(
            command.as_std_mut(),
            HelperMode::Guardian,
            lease_path,
            lease_token,
        )?;
        command.process_group(0);
        Ok(command)
    }

    fn anchor_command(&self, lease_path: &Path, lease_token: &str) -> io::Result<Command> {
        let mut command = Command::new(self.executable.launch_path());
        self.configure(&mut command, HelperMode::Anchor, lease_path, lease_token)?;
        Ok(command)
    }

    fn configure(
        &self,
        command: &mut Command,
        mode: HelperMode,
        lease_path: &Path,
        lease_token: &str,
    ) -> io::Result<()> {
        self.executable.configure_std_command(command)?;
        command.env_clear();
        if self.test_harness {
            command
                .args([
                    OsStr::new("--exact"),
                    OsStr::new("runtime::tests::provider_guardian_entry"),
                    OsStr::new("--nocapture"),
                ])
                .env(
                    TEST_MODE_ENV,
                    match mode {
                        HelperMode::Guardian => "guardian",
                        HelperMode::Anchor => "anchor",
                    },
                )
                .env(TEST_LEASE_ENV, lease_path)
                .env(TEST_TOKEN_ENV, lease_token);
        } else {
            command.arg(match mode {
                HelperMode::Guardian => GUARDIAN_FLAG,
                HelperMode::Anchor => ANCHOR_FLAG,
            });
            command.arg(lease_path);
            command.arg(lease_token);
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum HelperMode {
    Guardian,
    Anchor,
}

pub(crate) fn reexecution_path() -> io::Result<PathBuf> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        let _executable = std::fs::File::open("/proc/self/exe")?;
        Ok(PathBuf::from("/proc/self/exe"))
    }
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    {
        env::current_exe()
    }
}

pub fn run_process_helper_if_requested() -> Option<i32> {
    let mut arguments = env::args_os();
    let _ = arguments.next();
    let mode = match arguments.next().as_deref() {
        Some(value) if value == OsStr::new(GUARDIAN_FLAG) => HelperMode::Guardian,
        Some(value) if value == OsStr::new(ANCHOR_FLAG) => HelperMode::Anchor,
        _ => return None,
    };
    let Some(lease_path) = arguments.next().map(PathBuf::from) else {
        return Some(2);
    };
    let Some(lease_token) = arguments.next() else {
        return Some(2);
    };
    if arguments.next().is_some() {
        return Some(2);
    }
    let Ok(path) = reexecution_path() else {
        return Some(1);
    };
    let Ok(launch) = GuardianLaunch::production(&path) else {
        return Some(1);
    };
    Some(run_helper(
        mode,
        &lease_path,
        &lease_token.to_string_lossy(),
        &launch,
    ))
}

#[cfg(test)]
pub(crate) fn run_test_helper_if_requested() -> Option<i32> {
    let mode = match env::var(TEST_MODE_ENV).ok().as_deref() {
        Some("guardian") => HelperMode::Guardian,
        Some("anchor") => HelperMode::Anchor,
        _ => return None,
    };
    let Some(lease_path) = env::var_os(TEST_LEASE_ENV).map(PathBuf::from) else {
        return Some(2);
    };
    let Some(lease_token) = env::var_os(TEST_TOKEN_ENV) else {
        return Some(2);
    };
    let Ok(launch) = GuardianLaunch::test_harness() else {
        return Some(1);
    };
    Some(run_helper(
        mode,
        &lease_path,
        &lease_token.to_string_lossy(),
        &launch,
    ))
}

fn run_helper(
    mode: HelperMode,
    lease_path: &Path,
    lease_token: &str,
    launch: &GuardianLaunch,
) -> i32 {
    let result = match mode {
        HelperMode::Guardian => run_guardian(lease_path, lease_token, launch),
        HelperMode::Anchor => run_anchor(lease_path, lease_token),
    };
    i32::from(result.is_err())
}

fn run_guardian(lease_path: &Path, lease_token: &str, launch: &GuardianLaunch) -> io::Result<()> {
    let mut command = launch.anchor_command(lease_path, lease_token)?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut anchor = command.spawn()?;
    let anchor_pid = anchor.id();
    let anchor_input = anchor
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("provider anchor input is unavailable"))?;
    let operation = (|| {
        let anchor_output = anchor
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("provider anchor output is unavailable"))?;
        if read_ready(BufReader::new(anchor_output))? != anchor_pid {
            return Err(io::Error::other(
                "provider anchor readiness identity changed",
            ));
        }
        writeln!(io::stdout().lock(), "{READY_PREFIX}{anchor_pid}")?;
        io::stdout().lock().flush()?;
        let mut buffer = [0_u8; 1024];
        while io::stdin().lock().read(&mut buffer)? != 0 {}
        Ok(())
    })();
    let cleanup = terminate_runtime(&mut anchor, anchor_pid, lease_path, lease_token);
    drop(anchor_input);
    operation.and(cleanup)
}

fn run_anchor(lease_path: &Path, lease_token: &str) -> io::Result<()> {
    let pid = rustix::process::getpid();
    if rustix::process::getpgrp() != pid {
        return Err(io::Error::other(
            "provider anchor is not its process-group leader",
        ));
    }
    let _lease = crate::runtime_lease::activate_unix_runtime_lease(lease_path, lease_token, pid)?;
    writeln!(io::stdout().lock(), "{READY_PREFIX}{}", pid.as_raw_pid())?;
    io::stdout().lock().flush()?;
    let mut buffer = [0_u8; 1024];
    while io::stdin().lock().read(&mut buffer)? != 0 {}
    rustix::process::kill_process_group(pid, Signal::KILL)?;
    Err(io::Error::other(
        "provider anchor survived its own group kill",
    ))
}

fn read_ready(mut reader: impl BufRead) -> io::Result<u32> {
    let mut retained = 0_usize;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line)?;
        if count == 0 {
            return Err(io::Error::other("provider helper closed before readiness"));
        }
        retained = retained.saturating_add(count);
        if retained > MAX_HELPER_OUTPUT_BYTES {
            return Err(io::Error::other(
                "provider helper readiness exceeded its bound",
            ));
        }
        if let Some(value) = line.trim().strip_prefix(READY_PREFIX) {
            return value
                .parse::<u32>()
                .map_err(|_| io::Error::other("provider helper readiness is invalid"));
        }
    }
}

fn terminate_anchor(anchor: &mut Child, raw_pid: u32) -> io::Result<()> {
    let pid = i32::try_from(raw_pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("provider anchor pid is invalid"))?;
    let signal = rustix::process::kill_process_group(pid, Signal::KILL);
    let waited = anchor.wait();
    match (signal, waited) {
        (Ok(()), Ok(_)) => Ok(()),
        (_, Err(error)) => Err(error),
        (Err(error), _) => Err(error.into()),
    }
}

fn terminate_runtime(
    anchor: &mut Child,
    raw_pid: u32,
    lease_path: &Path,
    lease_token: &str,
) -> io::Result<()> {
    let pid = i32::try_from(raw_pid)
        .ok()
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("provider anchor pid is invalid"))?;
    let captured = CapturedRuntimeProcesses::freeze(lease_path, lease_token, pid);
    let anchor_result = terminate_anchor(anchor, raw_pid);
    let captured = match captured {
        Ok(captured) => captured,
        Err(error) => {
            let _ = anchor_result;
            return Err(error);
        }
    };
    let captured_result = captured
        .kill()
        .and_then(|()| captured.confirm_gone(lease_path, lease_token, Duration::from_secs(4)));
    anchor_result.and(captured_result)
}
