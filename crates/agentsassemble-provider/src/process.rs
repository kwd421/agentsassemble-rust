use std::{
    env, io,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use agentsassemble_domain::stable_identity_hash;
use same_file::Handle;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};
use tokio_util::sync::CancellationToken;

const PROBE_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_PROBE_STREAM_BYTES: usize = 2 * 1024 * 1024;
const PROBE_ENVIRONMENT: [&str; 18] = [
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "TMPDIR",
    "TMP",
    "TEMP",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_STATE_HOME",
    "APPDATA",
    "LOCALAPPDATA",
    "USERPROFILE",
    "SYSTEMROOT",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "LANG",
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
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    sanitize_environment(&mut command);
    ProbeTree::configure(&mut command);
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Err(ProbeFailure::Missing);
        }
        Err(_) => return Err(ProbeFailure::Failed),
    };
    let tree = match ProbeTree::attach(&mut child) {
        Ok(tree) => tree,
        Err(failure) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(failure);
        }
    };
    let (Some(stdout), Some(stderr)) = (child.stdout.take(), child.stderr.take()) else {
        terminate_probe_tree(&mut child, tree).await;
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
    terminate_probe_tree(&mut child, tree).await;
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

pub(crate) fn resolve_executable(program: &str) -> Option<(String, String)> {
    let path = env::var_os("PATH")?;
    for directory in env::split_paths(&path) {
        for candidate in executable_candidates(&directory, program) {
            if is_executable_file(&candidate)
                && let Ok(canonical) = candidate.canonicalize()
                && let Ok(canonical) = canonical.into_os_string().into_string()
                && let Ok(handle) = Handle::from_path(&canonical)
            {
                return Some((canonical, stable_identity_hash(&handle)));
            }
        }
    }
    None
}

fn sanitize_environment(command: &mut Command) {
    command.env_clear();
    for name in PROBE_ENVIRONMENT {
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

fn executable_candidates(directory: &Path, program: &str) -> Vec<PathBuf> {
    let base = directory.join(program);
    #[cfg(windows)]
    {
        if Path::new(program).extension().is_some() {
            return vec![base];
        }
        let extensions = env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_owned());
        extensions
            .split(';')
            .filter(|extension| !extension.is_empty())
            .map(|extension| directory.join(format!("{program}{extension}")))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec![base]
    }
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(unix)]
struct ProbeTree {
    process_group: i32,
}

#[cfg(unix)]
impl ProbeTree {
    fn configure(command: &mut Command) {
        command.process_group(0);
    }

    fn attach(child: &mut tokio::process::Child) -> Result<Self, ProbeFailure> {
        let process_group = child
            .id()
            .and_then(|pid| i32::try_from(pid).ok())
            .ok_or(ProbeFailure::Failed)?;
        Ok(Self { process_group })
    }
}

#[cfg(windows)]
struct ProbeTree {
    job: Option<win32job::Job>,
}

#[cfg(windows)]
impl ProbeTree {
    fn configure(_command: &mut Command) {}

    fn attach(child: &mut tokio::process::Child) -> Result<Self, ProbeFailure> {
        let job = win32job::Job::create().map_err(|_| ProbeFailure::Failed)?;
        let mut limits = job
            .query_extended_limit_info()
            .map_err(|_| ProbeFailure::Failed)?;
        limits.limit_kill_on_job_close();
        job.set_extended_limit_info(&limits)
            .map_err(|_| ProbeFailure::Failed)?;
        let handle = child.raw_handle().ok_or(ProbeFailure::Failed)? as isize;
        job.assign_process(handle)
            .map_err(|_| ProbeFailure::Failed)?;
        Ok(Self { job: Some(job) })
    }
}

#[cfg(not(any(unix, windows)))]
struct ProbeTree;

#[cfg(not(any(unix, windows)))]
impl ProbeTree {
    fn configure(_command: &mut Command) {}

    fn attach(_child: &mut tokio::process::Child) -> Result<Self, ProbeFailure> {
        Err(ProbeFailure::Failed)
    }
}

async fn terminate_probe_tree(child: &mut tokio::process::Child, tree: ProbeTree) {
    #[cfg(unix)]
    {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(-tree.process_group),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    #[cfg(windows)]
    drop(tree.job);
    #[cfg(not(any(unix, windows)))]
    let _ = tree;
    let _ = child.kill().await;
    let _ = child.wait().await;
}

#[cfg(all(test, unix))]
#[path = "process_tests.rs"]
mod tests;
