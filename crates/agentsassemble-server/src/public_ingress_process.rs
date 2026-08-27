use std::{
    env,
    ffi::OsString,
    io,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tempfile::TempDir;
use tokio::{
    process::{ChildStderr, ChildStdout},
    time::timeout,
};
use tokio_util::sync::CancellationToken;

const GRACEFUL_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const FORCED_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const NETWORK_ENVIRONMENT: [&str; 9] = [
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "SSL_CERT_DIR",
    "SSL_CERT_FILE",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "TMPDIR",
];
pub(crate) struct SpawnedTunnel {
    pub(crate) child: Box<dyn ChildWrapper>,
    pub(crate) stdout: ChildStdout,
    pub(crate) stderr: ChildStderr,
    _config_root: TempDir,
}

pub(crate) async fn spawn_cloudflared(
    executable: &Path,
    local_url: &str,
    origin_host: &str,
) -> io::Result<SpawnedTunnel> {
    let config_root = tempfile::tempdir()?;
    let config_path = config_root.path().join("config.yml");
    std::fs::write(&config_path, [])?;
    let mut command = owned_command(executable);
    command
        .command_mut()
        .args([
            "tunnel",
            "--no-autoupdate",
            "--config",
            config_path.to_string_lossy().as_ref(),
            "--url",
            local_url,
            "--http-host-header",
            origin_host,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    inherit_environment(&mut command, &NETWORK_ENVIRONMENT);
    let mut child = command.spawn()?;
    let Some(stdout) = child.stdout().take() else {
        terminate_now(child.as_mut()).await;
        return Err(io::Error::other("cloudflared stdout pipe is unavailable"));
    };
    let Some(stderr) = child.stderr().take() else {
        terminate_now(child.as_mut()).await;
        return Err(io::Error::other("cloudflared stderr pipe is unavailable"));
    };
    Ok(SpawnedTunnel {
        child,
        stdout,
        stderr,
        _config_root: config_root,
    })
}

pub(crate) async fn run_owned_command(
    executable: &Path,
    arguments: &[OsString],
    environment: &[&str],
    cancellation: &CancellationToken,
    active_timeout: Duration,
) -> io::Result<OwnedCommandOutcome> {
    let mut command = owned_command(executable);
    command
        .command_mut()
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    inherit_environment(&mut command, environment);
    let mut child = command.spawn()?;
    tokio::select! {
        result = child.wait() => result.map(OwnedCommandOutcome::Exited),
        () = cancellation.cancelled() => {
            Ok(if terminate_now(child.as_mut()).await {
                OwnedCommandOutcome::Cancelled
            } else {
                OwnedCommandOutcome::CleanupFailed
            })
        }
        () = tokio::time::sleep(active_timeout) => {
            Ok(if terminate_now(child.as_mut()).await {
                OwnedCommandOutcome::TimedOut
            } else {
                OwnedCommandOutcome::CleanupFailed
            })
        }
    }
}

pub(crate) enum OwnedCommandOutcome {
    Exited(ExitStatus),
    Cancelled,
    TimedOut,
    CleanupFailed,
}

fn owned_command(executable: &Path) -> CommandWrap {
    let mut command = CommandWrap::with_new(executable, |_| {});
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command
}

fn inherit_environment(command: &mut CommandWrap, names: &[&str]) {
    command.command_mut().env_clear();
    for name in names {
        if let Some(value) = env::var_os(name) {
            command.command_mut().env(name, value);
        }
    }
}

pub(crate) async fn supervise_child(
    child: &mut dyn ChildWrapper,
    cancellation: &CancellationToken,
) -> io::Result<std::process::ExitStatus> {
    tokio::select! {
        result = child.wait() => return result,
        () = cancellation.cancelled() => {}
    }
    let _ = request_graceful_stop(child);
    if let Ok(result) = timeout(GRACEFUL_STOP_TIMEOUT, child.wait()).await {
        return result;
    }
    child.start_kill()?;
    timeout(FORCED_STOP_TIMEOUT, child.wait())
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "owned child did not exit"))?
}

#[cfg(unix)]
fn request_graceful_stop(child: &dyn ChildWrapper) -> io::Result<()> {
    use rustix::process::{Pid, Signal, kill_process_group};

    let pid = child
        .id()
        .and_then(|pid| i32::try_from(pid).ok())
        .and_then(Pid::from_raw)
        .ok_or_else(|| io::Error::other("owned child PID is unavailable"))?;
    Ok(kill_process_group(pid, Signal::TERM)?)
}

#[cfg(not(unix))]
fn request_graceful_stop(child: &mut dyn ChildWrapper) -> io::Result<()> {
    child.start_kill()
}

async fn terminate_now(child: &mut dyn ChildWrapper) -> bool {
    if child.start_kill().is_err() {
        return false;
    }
    matches!(timeout(FORCED_STOP_TIMEOUT, child.wait()).await, Ok(Ok(_)))
}
