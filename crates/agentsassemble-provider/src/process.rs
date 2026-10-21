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
    /// A helper program the provider bridge needs, not the provider CLI itself.
    BridgeRuntimeMissing,
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

    let mut child = spawn_probe(program, args, environment, Stdio::null(), None)?;
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

/// Runs one command in `directory` with the same environment, tree custody
/// and cancellation as a probe, and the timeout the caller asked for, if any.
/// A non-zero exit is returned, not treated as a failure: the caller reports it.
pub(crate) async fn run_in_directory(
    program: &str,
    args: &[&str],
    directory: &std::path::Path,
    timeout: Option<Duration>,
    cancellation: &CancellationToken,
) -> Result<ProbeOutput, ProbeFailure> {
    if cancellation.is_cancelled() {
        return Err(ProbeFailure::Cancelled);
    }
    #[cfg(not(any(unix, windows)))]
    return Err(ProbeFailure::Failed);

    let mut child = spawn_probe(program, args, &[], Stdio::null(), Some(directory))?;
    #[cfg(unix)]
    let _group = GroupKill(
        child
            .id()
            .and_then(|id| i32::try_from(id).ok())
            .and_then(rustix::process::Pid::from_raw),
    );
    let (Some(stdout), Some(stderr)) = (child.stdout().take(), child.stderr().take()) else {
        terminate_probe_tree(child.as_mut()).await?;
        return Err(ProbeFailure::Failed);
    };
    // A long build runs to the end unless a timeout was asked for; the turn
    // interrupt (the cancellation) still stops it. Only the end of a long
    // log is kept, where a build or test failure usually is. The command is
    // over when its shell exits: anything it left running in the background
    // may hold the pipes open forever, so its output is read only briefly
    // after that, and the process group is killed when this returns.
    let (out, err) = (TailBuffer::default(), TailBuffer::default());
    let mut readers = [
        tokio::spawn(read_tail_into(stdout, out.clone())),
        tokio::spawn(read_tail_into(stderr, err.clone())),
    ];
    let run = async {
        let status = child.wait().await?;
        let drained = tokio::time::timeout(AFTER_EXIT_DRAIN, async {
            for reader in &mut readers {
                let _ = reader.await;
            }
        })
        .await;
        if drained.is_err() {
            readers.iter().for_each(tokio::task::JoinHandle::abort);
        }
        Ok::<_, io::Error>((out.take(), err.take(), status))
    };
    let collected = tokio::select! {
        () = cancellation.cancelled() => None,
        collected = Box::pin(async {
            match timeout {
                Some(timeout) => tokio::time::timeout(timeout, run).await,
                None => Ok(run.await),
            }
        }) => Some(collected),
    };
    match collected {
        Some(Ok(Ok((stdout, stderr, status)))) => Ok(ProbeOutput {
            status,
            stdout,
            stderr,
        }),
        None => {
            terminate_probe_tree(child.as_mut()).await?;
            Err(ProbeFailure::Cancelled)
        }
        Some(Ok(Err(_))) => {
            terminate_probe_tree(child.as_mut()).await?;
            Err(ProbeFailure::Failed)
        }
        Some(Err(_)) => {
            terminate_probe_tree(child.as_mut()).await?;
            Err(ProbeFailure::Timeout)
        }
    }
}

/// Kills a command's whole process group when its run ends or is dropped by a
/// turn interrupt. `KillOnDrop` reaches only the shell, so without this the
/// programs it started (and anything it left in the background) would outlive it.
#[cfg(unix)]
struct GroupKill(Option<rustix::process::Pid>);

#[cfg(unix)]
impl Drop for GroupKill {
    fn drop(&mut self) {
        if let Some(group) = self.0 {
            // An already empty group is the normal case after a clean exit.
            let _gone = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
        }
    }
}

fn spawn_probe(
    program: &str,
    args: &[&str],
    environment: &[(String, String)],
    stdin: Stdio,
    current_dir: Option<&std::path::Path>,
) -> Result<Box<dyn process_wrap::tokio::ChildWrapper>, ProbeFailure> {
    let mut command = CommandWrap::with_new(program, |command| {
        command
            .args(args)
            .stdin(stdin)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(directory) = current_dir {
            command.current_dir(directory);
        }
    });
    sanitize_environment(command.command_mut());
    command
        .command_mut()
        .envs(environment.iter().map(|(name, value)| (name, value)));
    command.wrap(KillOnDrop);
    #[cfg(unix)]
    command.wrap(ProcessGroup::leader());
    #[cfg(windows)]
    command
        .wrap(process_wrap::tokio::CreationFlags(
            windows::Win32::System::Threading::CREATE_NO_WINDOW,
        ))
        .wrap(JobObject);
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
    let mut child = spawn_probe(program, args, environment, Stdio::piped(), None)?;
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

/// How long a command's output is still read after its shell exits.
const AFTER_EXIT_DRAIN: Duration = Duration::from_secs(2);

/// The last `MAX_PROBE_STREAM_BYTES` of a stream, shared with its reader task.
#[derive(Clone, Default)]
struct TailBuffer(std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<u8>>>);

impl TailBuffer {
    fn push(&self, bytes: &[u8]) {
        let mut output = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        output.extend(bytes);
        let excess = output.len().saturating_sub(MAX_PROBE_STREAM_BYTES);
        output.drain(..excess);
    }

    fn take(&self) -> Vec<u8> {
        std::mem::take(
            &mut *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        )
        .into()
    }
}

async fn read_tail_into<R: AsyncRead + Unpin>(mut reader: R, output: TailBuffer) -> io::Result<()> {
    let mut chunk = vec![0_u8; 16 * 1024];
    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(());
        }
        output.push(&chunk[..count]);
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
