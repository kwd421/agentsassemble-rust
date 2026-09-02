use std::{env, ffi::OsString, io, process::Stdio, time::Duration};

#[cfg(not(unix))]
use process_wrap::tokio::ChildWrapper;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(not(unix))]
const CHILD_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_PROBE_STREAM_BYTES: usize = 2 * 1024 * 1024;
const PROVIDER_ENVIRONMENT: [&str; 22] = [
    "APPDATA",
    "COLORTERM",
    "COMSPEC",
    "HOME",
    "LANG",
    "LC_ALL",
    "LOCALAPPDATA",
    "LOGNAME",
    "PATH",
    "PATHEXT",
    "SHELL",
    "SYSTEMROOT",
    "TEMP",
    "TERM",
    "TMP",
    "TMPDIR",
    "USER",
    "USERPROFILE",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeFailure {
    Missing,
    Timeout,
    Authentication,
    Malformed,
    Failed,
    Cancelled,
    CatalogTooLarge,
    CleanupUnconfirmed,
}

#[cfg(not(unix))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildStopFailure {
    Deadline,
    Unconfirmed,
}

#[cfg(not(unix))]
pub(crate) async fn stop_child(child: &mut dyn ChildWrapper) -> Result<(), ChildStopFailure> {
    match tokio::time::timeout(CHILD_STOP_TIMEOUT, Box::into_pin(child.kill())).await {
        Ok(Ok(())) => Ok(()),
        Ok(Err(_)) => Err(ChildStopFailure::Unconfirmed),
        Err(_) => Err(ChildStopFailure::Deadline),
    }
}

pub(crate) async fn probe(
    program: &str,
    args: &[&str],
    cancellation: &CancellationToken,
) -> Result<String, ProbeFailure> {
    probe_with_timeout(program, args, PROBE_TIMEOUT, cancellation).await
}

pub(crate) async fn probe_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancellation: &CancellationToken,
) -> Result<String, ProbeFailure> {
    if cancellation.is_cancelled() {
        return Err(ProbeFailure::Cancelled);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(ProbeFailure::Failed);

    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    sanitize_environment(command.command_mut());
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ProbeFailure::Missing);
        }
        Err(_) => return Err(ProbeFailure::Failed),
    };
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        terminate_probe_tree(child.as_mut()).await?;
        return Err(ProbeFailure::Failed);
    };
    let collected = tokio::select! {
        () = cancellation.cancelled() => None,
        collected = Box::pin(tokio::time::timeout(timeout, async {
            tokio::try_join!(read_limited(stdout), read_limited(stderr), child.wait())
        })) => Some(collected),
    };
    let output = match collected {
        Some(Ok(Ok(output))) => output,
        None => {
            terminate_probe_tree(child.as_mut()).await?;
            return Err(ProbeFailure::Cancelled);
        }
        Some(Ok(Err(_))) => {
            terminate_probe_tree(child.as_mut()).await?;
            return Err(ProbeFailure::Malformed);
        }
        Some(Err(_)) => {
            terminate_probe_tree(child.as_mut()).await?;
            return Err(ProbeFailure::Timeout);
        }
    };
    let (stdout, stderr, status) = output;
    if !status.success() {
        let diagnostic = String::from_utf8_lossy(&stderr).to_lowercase();
        return Err(
            if diagnostic.contains("login") || diagnostic.contains("auth") {
                ProbeFailure::Authentication
            } else {
                ProbeFailure::Failed
            },
        );
    }
    String::from_utf8(stdout).map_err(|_| ProbeFailure::Malformed)
}

pub(crate) fn sanitize_environment(command: &mut tokio::process::Command) {
    command.env_clear();
    command.envs(sanitized_environment());
}

#[cfg(unix)]
pub(crate) fn sanitize_std_environment(command: &mut std::process::Command) {
    command.env_clear();
    command.envs(sanitized_environment());
}

pub(crate) fn sanitized_environment() -> Vec<(OsString, OsString)> {
    let mut environment = Vec::with_capacity(PROVIDER_ENVIRONMENT.len());
    for name in PROVIDER_ENVIRONMENT {
        if let Some(value) = env::var_os(name) {
            environment.push((OsString::from(name), value));
        }
    }
    environment
}

async fn read_limited<R: AsyncRead + Unpin>(mut reader: R) -> io::Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut chunk = vec![0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(count) > MAX_PROBE_STREAM_BYTES {
            return Err(io::Error::other("provider probe output exceeded its limit"));
        }
        output.extend_from_slice(&chunk[..count]);
    }
}

async fn terminate_probe_tree(
    child: &mut dyn process_wrap::tokio::ChildWrapper,
) -> Result<(), ProbeFailure> {
    let _signal = child.start_kill();
    match tokio::time::timeout(PROBE_TIMEOUT, child.wait()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(_)) | Err(_) => Err(ProbeFailure::CleanupUnconfirmed),
    }
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
