use std::{env, io, process::Stdio, time::Duration};

#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(unix)]
use process_wrap::tokio::ProcessGroup;
use process_wrap::tokio::{CommandWrap, KillOnDrop};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
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
}

pub(crate) async fn probe(
    program: &str,
    args: &[&str],
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
        terminate_probe_tree(child.as_mut()).await;
        return Err(ProbeFailure::Failed);
    };
    let collected = tokio::select! {
        () = cancellation.cancelled() => None,
        collected = Box::pin(tokio::time::timeout(PROBE_TIMEOUT, async {
            tokio::try_join!(read_limited(stdout), read_limited(stderr), child.wait())
        })) => Some(collected),
    };
    let outcome = match collected {
        None => Err(ProbeFailure::Cancelled),
        Some(Ok(Ok(output))) => Ok(output),
        Some(Ok(Err(_))) => Err(ProbeFailure::Malformed),
        Some(Err(_)) => Err(ProbeFailure::Timeout),
    };
    terminate_probe_tree(child.as_mut()).await;
    let (stdout, stderr, status) = outcome?;
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
    for name in PROVIDER_ENVIRONMENT {
        if let Some(value) = env::var_os(name) {
            command.env(name, value);
        }
    }
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

async fn terminate_probe_tree(child: &mut dyn process_wrap::tokio::ChildWrapper) {
    let _ = Box::into_pin(child.kill()).await;
    let _ = child.wait().await;
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
