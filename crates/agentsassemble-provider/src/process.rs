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

use agentsassemble_domain::runtime_shutdown::PROVIDER_PROBE_TIMEOUT as PROBE_TIMEOUT;
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
    environment: &[(String, String)],
) -> Result<String, ProbeFailure> {
    probe_with_timeout(program, args, PROBE_TIMEOUT, cancellation, environment).await
}

pub(crate) async fn probe_with_timeout(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancellation: &CancellationToken,
    environment: &[(String, String)],
) -> Result<String, ProbeFailure> {
    let output = probe_output(program, args, timeout, cancellation, environment).await?;
    if !output.status.success() {
        return Err(ProbeFailure::Failed);
    }
    String::from_utf8(output.stdout).map_err(|_| ProbeFailure::Malformed)
}

// Private streams can include account data. Do not derive Debug or publish them.
pub(crate) struct ProbeOutput {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

pub(crate) async fn probe_output(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancellation: &CancellationToken,
    environment: &[(String, String)],
) -> Result<ProbeOutput, ProbeFailure> {
    if cancellation.is_cancelled() {
        return Err(ProbeFailure::Cancelled);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(ProbeFailure::Failed);

    let mut child = spawn_probe(program, args, environment, Stdio::null())?;
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
    Ok(ProbeOutput {
        status,
        stdout,
        stderr,
    })
}

fn spawn_probe(
    program: &str,
    args: &[&str],
    environment: &[(String, String)],
    stdin: Stdio,
) -> Result<Box<dyn process_wrap::tokio::ChildWrapper>, ProbeFailure> {
    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(args)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    });
    sanitize_environment(command.command_mut());
    command
        .command_mut()
        .envs(environment.iter().map(|(name, value)| (name, value)));
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command.wrap(JobObject);
    match command.spawn() {
        Ok(child) => Ok(child),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Err(ProbeFailure::Missing),
        Err(_) => Err(ProbeFailure::Failed),
    }
}

/// Runs one bounded native inspection, then confirms its process tree is gone.
/// The exchange owns only private pipes and cannot outlive this process owner.
pub(crate) async fn inspect<T, F, Fut>(
    program: &str,
    args: &[&str],
    timeout: Duration,
    cancellation: &CancellationToken,
    environment: &[(String, String)],
    exchange: F,
) -> Result<T, ProbeFailure>
where
    F: FnOnce(tokio::process::ChildStdin, tokio::process::ChildStdout) -> Fut,
    Fut: std::future::Future<Output = Result<T, ProbeFailure>>,
{
    if cancellation.is_cancelled() {
        return Err(ProbeFailure::Cancelled);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(ProbeFailure::Failed);
    let mut child = spawn_probe(program, args, environment, Stdio::piped())?;
    let (Some(stdin), Some(stdout), Some(stderr)) = (
        child.stdin().take(),
        child.stdout().take(),
        child.stderr().take(),
    ) else {
        terminate_probe_tree(child.as_mut()).await?;
        return Err(ProbeFailure::Failed);
    };
    let drain_errors = async {
        read_limited(stderr)
            .await
            .map_err(|_| ProbeFailure::Malformed)?;
        std::future::pending::<Result<T, ProbeFailure>>().await
    };
    let outcome = tokio::select! {
        biased;
        () = cancellation.cancelled() => Err(ProbeFailure::Cancelled),
        result = tokio::time::timeout(timeout, exchange(stdin, stdout)) =>
            result.map_err(|_| ProbeFailure::Timeout).and_then(std::convert::identity),
        result = drain_errors => result,
    };
    terminate_probe_tree(child.as_mut()).await?;
    outcome
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
