use std::{fs::File, os::fd::OwnedFd, path::PathBuf, process::Stdio, time::Duration};

use futures_util::StreamExt;
use rustix::process::Pid;
use tokio::{io::AsyncWriteExt, process::ChildStdin};
use tokio_util::codec::{FramedRead, LinesCodec};

use crate::{
    filesystem::BoundExecutable,
    guardian::GuardianLaunch,
    runtime::DriverError,
    runtime_lease::{HeldRuntimeLease, provider_lifetime_is_active},
    unix_process_tree::tagged_runtime_exists,
};

const HELPER_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_HELPER_LINE_BYTES: usize = 1024;
const MAX_HELPER_LINES: usize = 32;
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_READY=";
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAX_PROCESS_STAT_BYTES: u64 = 4 * 1024;

pub(crate) struct UnixProviderPipes {
    pub(crate) stdin: tokio::fs::File,
    pub(crate) stdout: tokio::fs::File,
    pub(crate) stderr: tokio::fs::File,
}

pub(crate) struct UnixProcessCustody {
    anchor_pid: Pid,
    provider_pid: Pid,
    guardian: tokio::process::Child,
    guardian_input: Option<ChildStdin>,
    lease_path: PathBuf,
    runtime_token: String,
    armed: bool,
}

impl UnixProcessCustody {
    pub(crate) async fn start(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        provider: &BoundExecutable,
        provider_arguments: &[String],
    ) -> Result<(Self, UnixProviderPipes), DriverError> {
        let (provider_stdin, stdin) = provider_pipe()?;
        let (stdout, provider_stdout) = provider_pipe()?;
        let (stderr, provider_stderr) = provider_pipe()?;
        let mut command = launch
            .guardian_command(
                runtime_lease.path(),
                runtime_lease.token(),
                provider,
                provider_arguments,
                [provider_stdin, provider_stdout, provider_stderr],
            )
            .map_err(|_| custody_error())?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut guardian) = command.spawn() else {
            return Err(custody_error());
        };
        let Some(guardian_input) = guardian.stdin.take() else {
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        let Some(guardian_output) = guardian.stdout.take() else {
            drop(guardian_input);
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        let Ok(Ok((anchor_pid, provider_pid))) = tokio::time::timeout(
            HELPER_TIMEOUT,
            read_ready(FramedRead::new(
                guardian_output,
                LinesCodec::new_with_max_length(MAX_HELPER_LINE_BYTES),
            )),
        )
        .await
        else {
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        };
        if !matches!(
            provider_lifetime_is_active(runtime_lease.path(), runtime_lease.token()),
            Ok(true)
        ) {
            drop(guardian_input);
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        }
        if rustix::process::getpgid(Some(provider_pid)).map_err(|_| custody_error())? != anchor_pid
        {
            drop(guardian_input);
            terminate_failed_guardian(&mut guardian).await;
            return Err(custody_error());
        }
        Ok((
            Self {
                anchor_pid,
                provider_pid,
                guardian,
                guardian_input: Some(guardian_input),
                lease_path: runtime_lease.path().to_path_buf(),
                runtime_token: runtime_lease.token().to_owned(),
                armed: true,
            },
            UnixProviderPipes {
                stdin: tokio::fs::File::from_std(File::from(stdin)),
                stdout: tokio::fs::File::from_std(File::from(stdout)),
                stderr: tokio::fs::File::from_std(File::from(stderr)),
            },
        ))
    }

    pub(crate) fn leader_is_running(&mut self) -> Result<bool, DriverError> {
        if self
            .guardian
            .try_wait()
            .map_err(|_| health_error())?
            .is_some()
        {
            return Err(health_error());
        }
        match rustix::process::getpgid(Some(self.provider_pid)) {
            Ok(group) if group != self.anchor_pid => Ok(false),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            Ok(_) => provider_process_is_running(self.provider_pid).map_err(|_| health_error()),
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::SRCH) => Ok(false),
            Err(_) => Err(health_error()),
        }
    }

    pub(crate) async fn stop(&mut self) -> Result<(), DriverError> {
        self.request_stop();
        tokio::time::timeout(HELPER_TIMEOUT, async {
            let guardian = self.guardian.wait().await.map_err(|_| stop_error())?;
            if !guardian.success() {
                return Err(stop_error());
            }
            wait_for_group_absence(self.anchor_pid).await?;
            match tagged_runtime_exists(&self.runtime_token) {
                Ok(false)
                    if matches!(
                        provider_lifetime_is_active(&self.lease_path, &self.runtime_token),
                        Ok(false)
                    ) =>
                {
                    Ok(())
                }
                _ => Err(stop_error()),
            }
        })
        .await
        .map_err(|_| stop_error())??;
        self.armed = false;
        Ok(())
    }

    pub(crate) fn request_stop(&mut self) {
        if self.armed {
            drop(self.guardian_input.take());
        }
    }
}

async fn wait_for_group_absence(process_group: Pid) -> Result<(), DriverError> {
    loop {
        match rustix::process::test_kill_process_group(process_group) {
            Err(rustix::io::Errno::SRCH) => return Ok(()),
            Ok(()) | Err(rustix::io::Errno::PERM) => {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            Err(_) => return Err(stop_error()),
        }
    }
}

impl Drop for UnixProcessCustody {
    fn drop(&mut self) {
        self.request_stop();
    }
}

async fn read_ready(
    mut lines: FramedRead<tokio::process::ChildStdout, LinesCodec>,
) -> Result<(Pid, Pid), DriverError> {
    for _ in 0..MAX_HELPER_LINES {
        let line = lines
            .next()
            .await
            .ok_or_else(custody_error)?
            .map_err(|_| custody_error())?;
        if let Some(value) = line.strip_prefix(READY_PREFIX) {
            let Some((anchor, provider)) = value.split_once(':') else {
                return Err(custody_error());
            };
            let anchor = anchor
                .parse::<i32>()
                .ok()
                .and_then(Pid::from_raw)
                .ok_or_else(custody_error)?;
            let provider = provider
                .parse::<i32>()
                .ok()
                .and_then(Pid::from_raw)
                .ok_or_else(custody_error)?;
            return Ok((anchor, provider));
        }
    }
    Err(custody_error())
}

fn provider_pipe() -> Result<(OwnedFd, OwnedFd), DriverError> {
    std::io::pipe()
        .map(|(reader, writer)| (reader.into(), writer.into()))
        .map_err(|_| custody_error())
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn provider_process_is_running(process: Pid) -> std::io::Result<bool> {
    use std::io::Read;

    let path = format!("/proc/{}/stat", process.as_raw_pid());
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    let mut contents = String::new();
    file.take(MAX_PROCESS_STAT_BYTES + 1)
        .read_to_string(&mut contents)?;
    if contents.len() as u64 > MAX_PROCESS_STAT_BYTES {
        return Err(std::io::Error::other(
            "provider process status exceeded its bound",
        ));
    }
    process_stat_is_running(&contents)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn process_stat_is_running(contents: &str) -> std::io::Result<bool> {
    let state = contents
        .rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().next())
        .ok_or_else(|| std::io::Error::other("provider process status is invalid"))?;
    Ok(!matches!(state, "Z" | "X" | "x"))
}

async fn terminate_failed_guardian(guardian: &mut tokio::process::Child) {
    if let Some(mut input) = guardian.stdin.take() {
        let _ = input.shutdown().await;
    }
    let _ = guardian.kill().await;
    let _ = guardian.wait().await;
}

const fn custody_error() -> DriverError {
    DriverError::new(
        "provider_custody_unavailable",
        "The provider process custody helper could not be established.",
    )
}

const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_unconfirmed",
        "The Codex app-server process tree shutdown could not be confirmed.",
    )
}

const fn health_error() -> DriverError {
    DriverError::new(
        "provider_health_unknown",
        "The Codex app-server leader state could not be observed.",
    )
}

#[cfg(all(test, any(target_os = "linux", target_os = "android")))]
mod tests {
    #[test]
    fn process_status_rejects_zombies_and_dead_tasks() {
        assert!(
            super::process_stat_is_running("123 (provider worker) R 1 2 3")
                .unwrap_or_else(|error| panic!("parse running provider status: {error}"))
        );
        for state in ["Z", "X", "x"] {
            assert!(
                !super::process_stat_is_running(&format!("123 (provider worker) {state} 1 2 3"))
                    .unwrap_or_else(|error| panic!("parse stopped provider status: {error}"))
            );
        }
    }
}
