use std::{
    env,
    ffi::OsStr,
    fs::OpenOptions,
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

use fs2::FileExt;
use rustix::process::{Pid, Signal};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

const GUARDIAN_FLAG: &str = "--agentsassemble-provider-guardian";
const ANCHOR_FLAG: &str = "--agentsassemble-provider-anchor";
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_ANCHOR=";
const MAX_HELPER_OUTPUT_BYTES: usize = 8 * 1024;
const TEST_MODE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_MODE";
const TEST_LEASE_ENV: &str = "AGENTSASSEMBLE_INTERNAL_GUARDIAN_LEASE";

#[derive(Clone)]
pub(crate) struct GuardianLaunch {
    executable: PathBuf,
    test_harness: bool,
}

impl GuardianLaunch {
    pub(crate) fn production(executable: PathBuf) -> Self {
        Self {
            executable,
            test_harness: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn test_harness() -> io::Result<Self> {
        Ok(Self {
            executable: env::current_exe()?,
            test_harness: true,
        })
    }

    pub(crate) fn guardian_command(&self, lease_path: &Path) -> tokio::process::Command {
        let mut command = tokio::process::Command::new(&self.executable);
        self.configure(command.as_std_mut(), HelperMode::Guardian, lease_path);
        command
    }

    fn anchor_command(&self, lease_path: &Path) -> Command {
        let mut command = Command::new(&self.executable);
        self.configure(&mut command, HelperMode::Anchor, lease_path);
        command
    }

    fn configure(&self, command: &mut Command, mode: HelperMode, lease_path: &Path) {
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
                .env(TEST_LEASE_ENV, lease_path);
        } else {
            command.arg(match mode {
                HelperMode::Guardian => GUARDIAN_FLAG,
                HelperMode::Anchor => ANCHOR_FLAG,
            });
            command.arg(lease_path);
        }
    }
}

#[derive(Clone, Copy)]
enum HelperMode {
    Guardian,
    Anchor,
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
    if arguments.next().is_some() {
        return Some(2);
    }
    let launch = GuardianLaunch::production(match env::current_exe() {
        Ok(path) => path,
        Err(_) => return Some(1),
    });
    Some(run_helper(mode, &lease_path, &launch))
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
    let Ok(launch) = GuardianLaunch::test_harness() else {
        return Some(1);
    };
    Some(run_helper(mode, &lease_path, &launch))
}

fn run_helper(mode: HelperMode, lease_path: &Path, launch: &GuardianLaunch) -> i32 {
    let result = match mode {
        HelperMode::Guardian => run_guardian(lease_path, launch),
        HelperMode::Anchor => run_anchor(lease_path),
    };
    i32::from(result.is_err())
}

fn run_guardian(lease_path: &Path, launch: &GuardianLaunch) -> io::Result<()> {
    let mut command = launch.anchor_command(lease_path);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0);
    let mut anchor = command.spawn()?;
    let anchor_pid = anchor.id();
    let operation = (|| {
        let _anchor_input = anchor
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("provider anchor input is unavailable"))?;
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
    let cleanup = terminate_anchor(&mut anchor, anchor_pid);
    operation.and(cleanup)
}

fn run_anchor(lease_path: &Path) -> io::Result<()> {
    let pid = rustix::process::getpid();
    if rustix::process::getpgrp() != pid {
        return Err(io::Error::other(
            "provider anchor is not its process-group leader",
        ));
    }
    let lease = OpenOptions::new().read(true).write(true).open(lease_path)?;
    lease.try_lock_exclusive()?;
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
