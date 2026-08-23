use std::{
    fs::File,
    os::fd::{AsFd, OwnedFd},
    path::PathBuf,
    process::Stdio,
    time::Duration,
};

use futures_util::StreamExt;
use rustix::process::Pid;
use tokio::process::ChildStdin;
use tokio_util::codec::{FramedRead, LinesCodec};

use crate::{
    filesystem::BoundExecutable,
    guardian::{GuardianLaunch, ProviderForkPolicy, ProviderLaunchConfig},
    guardian_health,
    launch_error::DriverLaunchError,
    runtime::DriverError,
    runtime_lease::{
        HeldRuntimeLease, provider_lifetime_is_active, unix_cleanup_receipt_is_present,
    },
    unix_process_tree::tagged_runtime_exists,
};

const HELPER_TIMEOUT: Duration = Duration::from_secs(5);
const FAILED_START_CLEANUP_TIMEOUT: Duration = Duration::from_secs(7);
const MAX_HELPER_LINE_BYTES: usize = 1024;
const MAX_HELPER_LINES: usize = 32;
const READY_PREFIX: &str = "AGENTSASSEMBLE_PROVIDER_READY=";
#[cfg(any(target_os = "linux", target_os = "android"))]
const MAX_PROCESS_STAT_BYTES: u64 = 4 * 1024;

#[cfg(all(test, target_os = "linux"))]
static WAIT_FOR_TEST_ESCAPE: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn enable_test_escape_wait() {
    WAIT_FOR_TEST_ESCAPE.store(true, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn test_escape_pid_path(token: &str) -> PathBuf {
    std::env::temp_dir().join(format!("agentsassemble-launch-escape-{token}.pid"))
}

pub(crate) struct UnixProviderPipes {
    pub(crate) stdin: tokio::fs::File,
    pub(crate) stdout: tokio::fs::File,
    pub(crate) stderr: tokio::fs::File,
}

pub(crate) struct UnixProviderPty {
    pub(crate) terminal: tokio::io::unix::AsyncFd<OwnedFd>,
}

#[cfg(all(test, target_os = "linux"))]
async fn wait_for_test_escape(provider: Pid, anchor: Pid, token: &str) {
    if !WAIT_FOR_TEST_ESCAPE.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let pid_path = test_escape_pid_path(token);
    for _ in 0..2_000 {
        let escaped = pid_path.exists()
            && !matches!(rustix::process::getpgid(Some(provider)), Ok(group) if group == anchor);
        if escaped {
            return;
        }
        tokio::time::sleep(Duration::from_millis(1)).await;
    }
}

pub(crate) struct UnixProcessCustody {
    anchor_pid: Pid,
    provider_pid: Pid,
    guardian: tokio::process::Child,
    guardian_input: Option<ChildStdin>,
    guardian_output: FramedRead<tokio::process::ChildStdout, LinesCodec>,
    health_request_id: u64,
    health_poisoned: bool,
    lease_path: PathBuf,
    runtime_token: String,
    armed: bool,
}

impl UnixProcessCustody {
    #[cfg(all(test, target_os = "linux"))]
    pub(crate) async fn start(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        provider: &BoundExecutable,
        provider_arguments: &[String],
        provider_environment: &[(String, String)],
        working_directory: &std::path::Path,
    ) -> Result<(Self, UnixProviderPipes), DriverLaunchError> {
        Self::start_pipes(
            runtime_lease,
            launch,
            provider,
            provider_arguments,
            provider_environment,
            working_directory,
            ProviderForkPolicy::Deny,
        )
        .await
    }

    pub(crate) async fn start_with_children(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        provider: &BoundExecutable,
        provider_arguments: &[String],
        provider_environment: &[(String, String)],
        working_directory: &std::path::Path,
    ) -> Result<(Self, UnixProviderPipes), DriverLaunchError> {
        Self::start_pipes(
            runtime_lease,
            launch,
            provider,
            provider_arguments,
            provider_environment,
            working_directory,
            if provider.allows_child_processes() {
                ProviderForkPolicy::AllowInGroup
            } else {
                ProviderForkPolicy::Deny
            },
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_pipes(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        provider: &BoundExecutable,
        provider_arguments: &[String],
        provider_environment: &[(String, String)],
        working_directory: &std::path::Path,
        fork_policy: ProviderForkPolicy,
    ) -> Result<(Self, UnixProviderPipes), DriverLaunchError> {
        let (provider_stdin, stdin) = provider_pipe()?;
        let (stdout, provider_stdout) = provider_pipe()?;
        let (stderr, provider_stderr) = provider_pipe()?;
        Self::start_with_config(
            runtime_lease,
            launch,
            ProviderLaunchConfig {
                provider,
                arguments: provider_arguments,
                environment: provider_environment,
                working_directory,
                pipes: [provider_stdin, provider_stdout, provider_stderr],
                fork_policy,
            },
        )
        .await
        .map(|custody| {
            (
                custody,
                UnixProviderPipes {
                    stdin: tokio::fs::File::from_std(File::from(stdin)),
                    stdout: tokio::fs::File::from_std(File::from(stdout)),
                    stderr: tokio::fs::File::from_std(File::from(stderr)),
                },
            )
        })
    }

    pub(crate) async fn start_pty(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        provider: &BoundExecutable,
        provider_arguments: &[String],
        provider_environment: &[(String, String)],
        working_directory: &std::path::Path,
    ) -> Result<(Self, UnixProviderPty), DriverLaunchError> {
        let window = nix::pty::Winsize {
            ws_row: 40,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let opened = nix::pty::openpty(Some(&window), None).map_err(|_| custody_error())?;
        let provider_stdin = opened.slave.try_clone().map_err(|_| custody_error())?;
        let provider_stdout = opened.slave.try_clone().map_err(|_| custody_error())?;
        let provider_stderr = opened.slave;
        let flags = rustix::fs::fcntl_getfl(opened.master.as_fd()).map_err(|_| custody_error())?;
        rustix::fs::fcntl_setfl(opened.master.as_fd(), flags | rustix::fs::OFlags::NONBLOCK)
            .map_err(|_| custody_error())?;
        let terminal = tokio::io::unix::AsyncFd::new(opened.master).map_err(|_| custody_error())?;
        Self::start_with_config(
            runtime_lease,
            launch,
            ProviderLaunchConfig {
                provider,
                arguments: provider_arguments,
                environment: provider_environment,
                working_directory,
                pipes: [provider_stdin, provider_stdout, provider_stderr],
                fork_policy: ProviderForkPolicy::AllowInGroup,
            },
        )
        .await
        .map(|custody| (custody, UnixProviderPty { terminal }))
    }

    async fn start_with_config(
        runtime_lease: &HeldRuntimeLease,
        launch: &GuardianLaunch,
        config: ProviderLaunchConfig<'_>,
    ) -> Result<Self, DriverLaunchError> {
        let mut command = launch
            .guardian_command(runtime_lease.path(), runtime_lease.token(), config)
            .map_err(|_| custody_error())?;
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let Ok(mut guardian) = command.spawn() else {
            return Err(custody_error().into());
        };
        let Some(guardian_input) = guardian.stdin.take() else {
            return Err(failed_started_guardian(&mut guardian, None, runtime_lease).await);
        };
        let Some(guardian_output) = guardian.stdout.take() else {
            return Err(failed_started_guardian(
                &mut guardian,
                Some(guardian_input),
                runtime_lease,
            )
            .await);
        };
        let mut guardian_output = FramedRead::new(
            guardian_output,
            LinesCodec::new_with_max_length(MAX_HELPER_LINE_BYTES),
        );
        let Ok(Ok((anchor_pid, provider_pid))) =
            tokio::time::timeout(HELPER_TIMEOUT, read_ready(&mut guardian_output)).await
        else {
            return Err(failed_started_guardian(
                &mut guardian,
                Some(guardian_input),
                runtime_lease,
            )
            .await);
        };
        #[cfg(all(test, target_os = "linux"))]
        wait_for_test_escape(provider_pid, anchor_pid, runtime_lease.token()).await;
        if !matches!(
            provider_lifetime_is_active(runtime_lease.path(), runtime_lease.token()),
            Ok(true)
        ) {
            return Err(failed_started_guardian(
                &mut guardian,
                Some(guardian_input),
                runtime_lease,
            )
            .await);
        }
        match rustix::process::getpgid(Some(provider_pid)) {
            Ok(group) if group == anchor_pid => {}
            Ok(_) | Err(_) => {
                return Err(failed_started_guardian(
                    &mut guardian,
                    Some(guardian_input),
                    runtime_lease,
                )
                .await);
            }
        }
        Ok(Self {
            anchor_pid,
            provider_pid,
            guardian,
            guardian_input: Some(guardian_input),
            guardian_output,
            health_request_id: 0,
            health_poisoned: false,
            lease_path: runtime_lease.path().to_path_buf(),
            runtime_token: runtime_lease.token().to_owned(),
            armed: true,
        })
    }

    pub(crate) async fn leader_is_running(&mut self) -> Result<bool, DriverError> {
        if self.health_poisoned {
            return Err(health_error());
        }
        if self
            .guardian
            .try_wait()
            .map_err(|_| health_error())?
            .is_some()
        {
            return Err(health_error());
        }
        let Some(request_id) = self.health_request_id.checked_add(1) else {
            self.health_poisoned = true;
            return Err(health_error());
        };
        self.health_request_id = request_id;
        let exact_child_is_alive = {
            let input = self.guardian_input.as_mut().ok_or_else(health_error)?;
            let mut poison = HealthProbePoison::new(&mut self.health_poisoned);
            let result = guardian_health::probe(
                input,
                &mut self.guardian_output,
                self.provider_pid,
                request_id,
            )
            .await;
            if result.is_ok() {
                poison.disarm();
            }
            result
        }?;
        if !exact_child_is_alive {
            return Ok(false);
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

struct HealthProbePoison<'a> {
    poisoned: &'a mut bool,
    armed: bool,
}

impl<'a> HealthProbePoison<'a> {
    fn new(poisoned: &'a mut bool) -> Self {
        Self {
            poisoned,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for HealthProbePoison<'_> {
    fn drop(&mut self) {
        if self.armed {
            *self.poisoned = true;
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
    lines: &mut FramedRead<tokio::process::ChildStdout, LinesCodec>,
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

async fn failed_started_guardian(
    guardian: &mut tokio::process::Child,
    guardian_input: Option<ChildStdin>,
    runtime_lease: &HeldRuntimeLease,
) -> DriverLaunchError {
    drop(guardian_input);
    let finished = tokio::time::timeout(FAILED_START_CLEANUP_TIMEOUT, guardian.wait()).await;
    let guardian_exited = matches!(&finished, Ok(Ok(_)));
    let receipt = guardian_exited
        && matches!(
            unix_cleanup_receipt_is_present(runtime_lease.path(), runtime_lease.token()),
            Ok(true)
        );
    if !guardian_exited {
        let _ = guardian.kill().await;
        let _ = guardian.wait().await;
    }
    if receipt {
        DriverLaunchError::safe(custody_error())
    } else {
        DriverLaunchError::uncertain(custody_error())
    }
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

#[cfg(test)]
mod tests {
    #[test]
    fn incomplete_health_probe_permanently_poisons_the_channel() {
        let mut poisoned = false;
        {
            let _probe = super::HealthProbePoison::new(&mut poisoned);
        }
        assert!(poisoned);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
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
