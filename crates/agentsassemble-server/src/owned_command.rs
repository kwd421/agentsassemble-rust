#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use std::{
    env,
    ffi::OsString,
    io,
    path::Path,
    process::{ExitStatus, Stdio},
    time::Duration,
};
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;
const FORCED_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub(crate) const NETWORK_ENVIRONMENT: [&str; 9] = [
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
pub(crate) async fn run_owned_command(
    executable: &Path,
    arguments: &[OsString],
    directory: Option<&Path>,
    additional_environment: &[&str],
    cancellation: &CancellationToken,
    active_timeout: Duration,
) -> io::Result<OwnedCommandOutcome> {
    if cancellation.is_cancelled() {
        return Ok(OwnedCommandOutcome::Cancelled);
    }
    let mut command = owned_command(executable);
    if let Some(directory) = directory {
        command.command_mut().current_dir(directory);
    }
    command
        .command_mut()
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    inherit_environment(&mut command, &NETWORK_ENVIRONMENT);
    inherit_selected_environment(&mut command, additional_environment);
    let mut child = command.spawn()?;
    tokio::select! {
        result = child.wait() => Ok(match result {
            Ok(status) => OwnedCommandOutcome::Exited(status),
            Err(_) if terminate_now(child.as_mut()).await => OwnedCommandOutcome::WaitFailed,
            Err(_) => OwnedCommandOutcome::CleanupFailed,
        }),
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
    WaitFailed,
    Cancelled,
    TimedOut,
    CleanupFailed,
}

pub(crate) fn owned_command(executable: &Path) -> CommandWrap {
    let mut command = CommandWrap::with_new(executable, |_| {});
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    command
}

pub(crate) fn inherit_environment(command: &mut CommandWrap, names: &[&str]) {
    command.command_mut().env_clear();
    inherit_selected_environment(command, names);
}

pub(crate) fn inherit_selected_environment(command: &mut CommandWrap, names: &[&str]) {
    for name in names {
        if let Some(value) = env::var_os(name) {
            command.command_mut().env(name, value);
        }
    }
}

pub(crate) async fn terminate_now(child: &mut dyn ChildWrapper) -> bool {
    let _ = child.start_kill();
    matches!(timeout(FORCED_STOP_TIMEOUT, child.wait()).await, Ok(Ok(_)))
}
