use std::{
    fs::File,
    io,
    path::Path,
    sync::{Arc, OnceLock},
    time::Duration,
};

use agentsassemble_domain::{
    AgentSessionDraft, stable_bundle_identity, stable_content_identity, stable_identity_hash,
};
use same_file::Handle;
use tokio::sync::{Semaphore, oneshot};

use crate::PersistenceError;

const FILESYSTEM_TIMEOUT: Duration = Duration::from_secs(10);
const FILESYSTEM_WORKERS: usize = 4;
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();
#[cfg(test)]
static TEST_REVALIDATION_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
    // Unit tests share this process-global production pool across independent
    // Tokio runtimes. Keep their unrelated authority checks from consuming one
    // another's production capacity while preserving the real four-worker gate.
    #[cfg(test)]
    let _test_revalidation_guard = TEST_REVALIDATION_LOCK.lock().await;

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
    if draft.workspace.is_empty() {
        if !draft.workspace_identity.is_empty() {
            return Err(io::Error::other("workspace authority is inconsistent"));
        }
    } else {
        let workspace = Path::new(&draft.workspace);
        let canonical_workspace = workspace.canonicalize()?;
        if canonical_workspace != workspace || !canonical_workspace.is_dir() {
            return Err(io::Error::other("workspace authority is not canonical"));
        }
        let workspace_identity = stable_identity_hash(&Handle::from_path(&canonical_workspace)?);
        if workspace_identity != draft.workspace_identity {
            return Err(io::Error::other("workspace identity changed"));
        }
    }

    if draft.executable.is_empty() {
        if !draft.executable_identity.is_empty() {
            return Err(io::Error::other("executable authority is inconsistent"));
        }
    } else {
        let executable = Path::new(&draft.executable);
        let canonical_executable = executable.canonicalize()?;
        if canonical_executable != executable {
            return Err(io::Error::other("executable authority is not canonical"));
        }
        let executable_identity =
            runtime_executable_identity(&draft.provider_kind, &canonical_executable)?;
        if executable_identity != draft.executable_identity {
            return Err(io::Error::other("executable identity changed"));
        }
    }
    Ok(())
}

fn runtime_executable_identity(provider_kind: &str, executable: &Path) -> io::Result<String> {
    let executable_identity = open_executable_identity(executable)?;
    if provider_kind != "codex_live_session" {
        return Ok(executable_identity);
    }
    let companion = executable
        .parent()
        .ok_or_else(|| io::Error::other("Codex executable directory is unavailable"))?
        .join(if cfg!(windows) {
            "codex-code-mode-host.exe"
        } else {
            "codex-code-mode-host"
        });
    if companion.canonicalize()? != companion {
        return Err(io::Error::other(
            "Codex code-mode host authority is not canonical",
        ));
    }
    let companion_identity = open_executable_identity(&companion)?;
    Ok(stable_bundle_identity(
        "codex-native",
        &[&executable_identity, &companion_identity],
    ))
}

fn open_executable_identity(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    if !is_executable_file(&file)? {
        return Err(io::Error::other("executable authority is not executable"));
    }
    let handle = Handle::from_file(file.try_clone()?)?;
    stable_content_identity(&handle, &mut file)
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
    use std::{io, path::Path, sync::Arc, time::Duration};

    use agentsassemble_domain::AgentSessionDraft;
    use tokio::sync::Semaphore;

    use super::{AuthorityFailure, run_with, runtime_executable_identity, validate_sync};

    fn api_draft() -> AgentSessionDraft {
        AgentSessionDraft {
            agent_id: "deepseek-test".to_owned(),
            display_name: "DeepSeek".to_owned(),
            provider_kind: "deepseek_api".to_owned(),
            runtime_kind: "api".to_owned(),
            connection_kind: "native_cli_bridge".to_owned(),
            executable: String::new(),
            executable_identity: String::new(),
            workspace: String::new(),
            workspace_identity: String::new(),
            model: "deepseek-v4-flash".to_owned(),
            reasoning_effort: "high".to_owned(),
            service_tier: "default".to_owned(),
            variant: "thinking".to_owned(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 4_096,
            catalog_revision: "catalog-1".to_owned(),
            persona_card_id: String::new(),
            runtime_profile_key: "profile-1".to_owned(),
            transport: "https".to_owned(),
        }
    }

    #[test]
    fn optional_authority_must_be_absent_as_a_complete_pair() {
        assert!(validate_sync(&api_draft()).is_ok());

        let mut inconsistent_workspace = api_draft();
        inconsistent_workspace.workspace_identity = "unexpected".to_owned();
        assert!(validate_sync(&inconsistent_workspace).is_err());

        let mut inconsistent_executable = api_draft();
        inconsistent_executable.executable_identity = "unexpected".to_owned();
        assert!(validate_sync(&inconsistent_executable).is_err());
    }

    fn write_executable(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).unwrap_or_else(|error| panic!("write executable: {error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path)
                .unwrap_or_else(|error| panic!("read executable mode: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(path, permissions)
                .unwrap_or_else(|error| panic!("set executable mode: {error}"));
        }
    }

    #[test]
    fn codex_authority_revalidates_the_complete_native_bundle() {
        let root =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create bundle root: {error}"));
        let executable = root
            .path()
            .join(if cfg!(windows) { "codex.exe" } else { "codex" });
        let companion = root.path().join(if cfg!(windows) {
            "codex-code-mode-host.exe"
        } else {
            "codex-code-mode-host"
        });
        write_executable(&executable, b"codex-main");
        write_executable(&companion, b"codex-host");
        let executable = executable
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize executable: {error}"));
        let first = runtime_executable_identity("codex_live_session", &executable)
            .unwrap_or_else(|error| panic!("identify Codex bundle: {error}"));
        assert!(first.starts_with("bundle-identity-v1-"));
        write_executable(&companion, b"changed-host");
        let changed = runtime_executable_identity("codex_live_session", &executable)
            .unwrap_or_else(|error| panic!("reidentify Codex bundle: {error}"));
        assert_ne!(first, changed);
    }

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
