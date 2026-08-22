use std::{
    fs::File,
    io,
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};

use agentsassemble_domain::{AgentSessionDraft, stable_content_identity, stable_identity_hash};
use same_file::Handle;
use tokio::sync::{Semaphore, oneshot};

use crate::PersistenceError;

const FILESYSTEM_TIMEOUT: Duration = Duration::from_secs(10);
const FILESYSTEM_WORKERS: usize = 4;
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq, Eq)]
enum AuthorityFailure {
    Busy,
    Timeout,
    Changed,
    Task(&'static str),
}

pub(crate) async fn revalidate_runtime_authority(
    draft: &AgentSessionDraft,
) -> Result<(), PersistenceError> {
    let draft = draft.clone();
    let outcome = run_with(
        Arc::clone(WORKERS.get_or_init(|| Arc::new(Semaphore::new(FILESYSTEM_WORKERS)))),
        FILESYSTEM_TIMEOUT,
        move || validate_sync(&draft),
    )
    .await;
    match outcome {
        Ok(()) => Ok(()),
        Err(AuthorityFailure::Busy) => Err(PersistenceError::CommandRejected {
            code: "runtime_authority_busy",
            message: "Runtime authority validation is at capacity.".to_owned(),
        }),
        Err(AuthorityFailure::Timeout) => Err(PersistenceError::CommandRejected {
            code: "runtime_authority_timeout",
            message: "Runtime authority validation timed out.".to_owned(),
        }),
        Err(AuthorityFailure::Changed) => Err(PersistenceError::CommandRejected {
            code: "runtime_authority_changed",
            message: "Workspace or provider executable authority changed before commit.".to_owned(),
        }),
        Err(AuthorityFailure::Task(message)) => {
            Err(PersistenceError::RuntimeAuthorityTask(message.to_owned()))
        }
    }
}

async fn run_with<F>(
    workers: Arc<Semaphore>,
    timeout: Duration,
    operation: F,
) -> Result<(), AuthorityFailure>
where
    F: FnOnce() -> io::Result<()> + Send + 'static,
{
    let permit = workers
        .try_acquire_owned()
        .map_err(|_| AuthorityFailure::Busy)?;
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("agentsassemble-persistence-fs".to_owned())
        .spawn(move || {
            let result = operation();
            drop(permit);
            let _ = sender.send(result);
        })
        .map_err(|_| AuthorityFailure::Task("filesystem worker could not be started"))?;
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(_))) => Err(AuthorityFailure::Changed),
        Ok(Err(_)) => Err(AuthorityFailure::Task(
            "filesystem worker result channel closed",
        )),
        Err(_) => Err(AuthorityFailure::Timeout),
    }
}

fn validate_sync(draft: &AgentSessionDraft) -> io::Result<()> {
    let workspace = Path::new(&draft.workspace);
    let canonical_workspace = workspace.canonicalize()?;
    if canonical_workspace != workspace || !canonical_workspace.is_dir() {
        return Err(io::Error::other("workspace authority is not canonical"));
    }
    let workspace_identity = stable_identity_hash(&Handle::from_path(&canonical_workspace)?);
    if workspace_identity != draft.workspace_identity {
        return Err(io::Error::other("workspace identity changed"));
    }

    let executable = Path::new(&draft.executable);
    let canonical_executable = executable.canonicalize()?;
    if canonical_executable != executable {
        return Err(io::Error::other("executable authority is not canonical"));
    }
    let mut executable_file = File::open(&canonical_executable)?;
    if !is_executable_file(&executable_file)? {
        return Err(io::Error::other("executable authority is not executable"));
    }
    let executable_handle = Handle::from_file(executable_file.try_clone()?)?;
    let executable_identity = stable_content_identity(&executable_handle, &mut executable_file)?;
    if executable_identity != draft.executable_identity {
        return Err(io::Error::other("executable identity changed"));
    }
    Ok(())
}

fn is_executable_file(file: &File) -> io::Result<bool> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(false);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        Ok(metadata.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use std::{io, sync::Arc, time::Duration};

    use tokio::sync::Semaphore;

    use super::{AuthorityFailure, run_with};

    #[test]
    fn stalled_worker_does_not_join_tokio_runtime_shutdown() {
        let workers = Arc::new(Semaphore::new(1));
        let worker_view = Arc::clone(&workers);
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let (finished_sender, finished_receiver) = std::sync::mpsc::channel();
        let (dropped_sender, dropped_receiver) = std::sync::mpsc::channel();
        let runtime_thread = std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
                .unwrap_or_else(|error| panic!("build test runtime: {error}"));
            let outcome = runtime.block_on(run_with(worker_view, Duration::ZERO, move || {
                release_receiver
                    .recv()
                    .map_err(|_| io::Error::other("test release sender closed"))?;
                finished_sender
                    .send(())
                    .map_err(|_| io::Error::other("test observer closed"))
            }));
            assert_eq!(outcome, Err(AuthorityFailure::Timeout));
            drop(runtime);
            dropped_sender
                .send(())
                .unwrap_or_else(|error| panic!("publish runtime shutdown: {error}"));
        });

        dropped_receiver
            .recv()
            .unwrap_or_else(|error| panic!("observe runtime shutdown: {error}"));
        assert!(workers.try_acquire().is_err());
        release_sender
            .send(())
            .unwrap_or_else(|error| panic!("release authority worker: {error}"));
        finished_receiver
            .recv()
            .unwrap_or_else(|error| panic!("observe authority worker cleanup: {error}"));
        runtime_thread
            .join()
            .unwrap_or_else(|_| panic!("runtime test thread panicked"));
    }
}
