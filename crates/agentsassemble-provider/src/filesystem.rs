use std::{
    env,
    fs::{File, OpenOptions},
    io::{self, Seek},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
    time::Duration,
};

use agentsassemble_domain::{stable_content_identity, stable_identity_hash};
use same_file::Handle;
use tokio::sync::{Semaphore, oneshot};

const FILESYSTEM_TIMEOUT: Duration = Duration::from_secs(10);
const FILESYSTEM_WORKERS: usize = 4;
static WORKERS: OnceLock<Arc<Semaphore>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FilesystemFailure {
    Busy,
    Timeout,
    Failed,
}

pub(crate) struct BoundExecutable {
    _file: File,
    launch_path: String,
    #[cfg(target_os = "macos")]
    _staging: tempfile::TempDir,
}

impl BoundExecutable {
    pub(crate) fn launch_path(&self) -> &str {
        &self.launch_path
    }

    #[cfg(unix)]
    #[cfg(not(target_os = "macos"))]
    pub(crate) fn configure_command(
        self,
        command: &mut tokio::process::Command,
    ) -> Result<(), FilesystemFailure> {
        use command_fds::{CommandFdExt, FdMapping};

        command
            .fd_mappings(vec![FdMapping {
                parent_fd: self._file.into(),
                child_fd: 3,
            }])
            .map(|_| ())
            .map_err(|_| FilesystemFailure::Failed)
    }
}

pub(crate) async fn resolve_executable(
    program: &str,
) -> Result<Option<(String, String)>, FilesystemFailure> {
    let program = program.to_owned();
    run_bounded(move || resolve_executable_sync(&program)).await
}

pub(crate) async fn canonical_workspace(
    workspace: String,
) -> Result<(String, String), FilesystemFailure> {
    run_bounded(move || {
        let canonical = std::fs::canonicalize(&workspace)?;
        if !canonical.is_dir() {
            return Err(io::Error::other("workspace is not a directory"));
        }
        let encoded = canonical
            .to_str()
            .ok_or_else(|| io::Error::other("workspace path is not UTF-8"))?
            .to_owned();
        let identity = stable_identity_hash(&Handle::from_path(&canonical)?);
        Ok((encoded, identity))
    })
    .await
}

pub(crate) async fn executable_identity(path: String) -> Result<String, FilesystemFailure> {
    run_bounded(move || executable_identity_sync(Path::new(&path))).await
}

pub(crate) async fn bind_executable(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    run_bounded(move || bind_executable_sync(Path::new(&path), &expected_identity)).await
}

async fn run_bounded<T, F>(operation: F) -> Result<T, FilesystemFailure>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    run_with(
        Arc::clone(WORKERS.get_or_init(|| Arc::new(Semaphore::new(FILESYSTEM_WORKERS)))),
        FILESYSTEM_TIMEOUT,
        operation,
    )
    .await
}

async fn run_with<T, F>(
    workers: Arc<Semaphore>,
    timeout: Duration,
    operation: F,
) -> Result<T, FilesystemFailure>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    let permit = workers
        .try_acquire_owned()
        .map_err(|_| FilesystemFailure::Busy)?;
    let (sender, receiver) = oneshot::channel();
    std::thread::Builder::new()
        .name("agentsassemble-provider-fs".to_owned())
        .spawn(move || {
            let result = operation();
            drop(permit);
            let _ = sender.send(result);
        })
        .map_err(|_| FilesystemFailure::Failed)?;
    match tokio::time::timeout(timeout, receiver).await {
        Ok(Ok(Ok(value))) => Ok(value),
        Ok(Ok(Err(_)) | Err(_)) => Err(FilesystemFailure::Failed),
        Err(_) => Err(FilesystemFailure::Timeout),
    }
}

fn resolve_executable_sync(program: &str) -> io::Result<Option<(String, String)>> {
    let Some(path) = env::var_os("PATH") else {
        return Ok(None);
    };
    for directory in env::split_paths(&path) {
        for candidate in executable_candidates(&directory, program) {
            if !is_executable_file(&candidate)? {
                continue;
            }
            let canonical = candidate.canonicalize()?;
            let identity = executable_identity_sync(&canonical)?;
            let encoded = canonical
                .to_str()
                .ok_or_else(|| io::Error::other("executable path is not UTF-8"))?;
            return Ok(Some((encoded.to_owned(), identity)));
        }
    }
    Ok(None)
}

fn executable_identity_sync(path: &Path) -> io::Result<String> {
    let canonical = path.canonicalize()?;
    if canonical != path || !is_executable_file(&canonical)? {
        return Err(io::Error::other("executable authority is not canonical"));
    }
    let mut file = File::open(&canonical)?;
    let handle = Handle::from_file(file.try_clone()?)?;
    stable_content_identity(&handle, &mut file)
}

fn bind_executable_sync(path: &Path, expected_identity: &str) -> io::Result<BoundExecutable> {
    let canonical = path.canonicalize()?;
    if canonical != path || !is_executable_file(&canonical)? {
        return Err(io::Error::other("executable authority is not canonical"));
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        options.share_mode(FILE_SHARE_READ);
    }
    let mut file = options.open(&canonical)?;
    let handle = Handle::from_file(file.try_clone()?)?;
    let identity = stable_content_identity(&handle, &mut file)?;
    file.rewind()?;
    if identity != expected_identity {
        return Err(io::Error::other("executable identity changed"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    let launch_path = "/dev/fd/3".to_owned();
    #[cfg(target_os = "macos")]
    let (launch_path, staging) = stage_macos_executable(&mut file, &handle, expected_identity)?;
    #[cfg(windows)]
    let launch_path = canonical
        .to_str()
        .ok_or_else(|| io::Error::other("executable path is not UTF-8"))?
        .to_owned();
    #[cfg(not(any(unix, windows)))]
    let launch_path = canonical
        .to_str()
        .ok_or_else(|| io::Error::other("executable path is not UTF-8"))?
        .to_owned();
    Ok(BoundExecutable {
        _file: file,
        launch_path,
        #[cfg(target_os = "macos")]
        _staging: staging,
    })
}

#[cfg(target_os = "macos")]
fn stage_macos_executable(
    source: &mut File,
    source_handle: &Handle,
    expected_identity: &str,
) -> io::Result<(String, tempfile::TempDir)> {
    use std::os::unix::fs::PermissionsExt;

    let staging = tempfile::Builder::new()
        .prefix("agentsassemble-provider-exec-")
        .tempdir()?;
    let staged_path = staging.path().join("provider");
    let mut staged = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&staged_path)?;
    source.rewind()?;
    io::copy(source, &mut staged)?;
    staged.sync_all()?;
    std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o500))?;
    source.rewind()?;
    if stable_content_identity(source_handle, &mut *source)? != expected_identity {
        return Err(io::Error::other("executable changed while it was staged"));
    }
    source.rewind()?;
    staged.rewind()?;
    if raw_content_digest(source)? != raw_content_digest(&mut staged)? {
        return Err(io::Error::other("staged executable bytes do not match"));
    }
    let launch_path = staged_path
        .to_str()
        .ok_or_else(|| io::Error::other("staged executable path is not UTF-8"))?
        .to_owned();
    Ok((launch_path, staging))
}

#[cfg(target_os = "macos")]
fn raw_content_digest(file: &mut File) -> io::Result<[u8; 32]> {
    use std::io::Read;

    use sha2::{Digest, Sha256};

    let mut digest = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(digest.finalize().into())
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

fn is_executable_file(path: &Path) -> io::Result<bool> {
    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
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

    use super::{FilesystemFailure, run_with};

    #[tokio::test]
    async fn stalled_filesystem_thread_times_out_without_releasing_capacity() {
        let workers = Arc::new(Semaphore::new(1));
        let (release_sender, release_receiver) = std::sync::mpsc::channel();
        let outcome = run_with(Arc::clone(&workers), Duration::ZERO, move || {
            release_receiver
                .recv()
                .map_err(|_| io::Error::other("test release sender closed"))
        })
        .await;
        assert_eq!(outcome, Err(FilesystemFailure::Timeout));
        assert!(workers.try_acquire().is_err());
        release_sender
            .send(())
            .unwrap_or_else(|error| panic!("release filesystem worker: {error}"));
        let _permit = workers
            .acquire()
            .await
            .unwrap_or_else(|error| panic!("reacquire filesystem worker: {error}"));
    }

    #[tokio::test]
    async fn executable_identity_detects_in_place_byte_changes() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create executable root: {error}"));
        let executable = directory.path().join("provider-test");
        std::fs::write(&executable, b"first provider bytes")
            .unwrap_or_else(|error| panic!("write executable: {error}"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(&executable)
                .unwrap_or_else(|error| panic!("read executable mode: {error}"))
                .permissions();
            permissions.set_mode(0o700);
            std::fs::set_permissions(&executable, permissions)
                .unwrap_or_else(|error| panic!("set executable mode: {error}"));
        }
        let canonical = executable
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize executable: {error}"));
        let first = super::executable_identity(canonical.to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("identify executable: {error:?}"));
        std::fs::write(&executable, b"changed provider bytes")
            .unwrap_or_else(|error| panic!("overwrite executable: {error}"));
        let changed = super::executable_identity(canonical.to_string_lossy().into_owned())
            .await
            .unwrap_or_else(|error| panic!("reidentify executable: {error:?}"));
        assert_ne!(first, changed);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bound_executable_launches_the_verified_open_file() {
        use std::os::unix::fs::PermissionsExt;

        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create bound root: {error}"));
        let executable = directory.path().join("bound-provider");
        std::fs::write(&executable, b"#!/bin/sh\nprintf 'verified-bytes'")
            .unwrap_or_else(|error| panic!("write bound executable: {error}"));
        let mut permissions = std::fs::metadata(&executable)
            .unwrap_or_else(|error| panic!("read bound mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&executable, permissions)
            .unwrap_or_else(|error| panic!("set bound mode: {error}"));
        let executable = executable
            .canonicalize()
            .unwrap_or_else(|error| panic!("canonicalize bound executable: {error}"))
            .to_string_lossy()
            .into_owned();
        let identity = super::executable_identity(executable.clone())
            .await
            .unwrap_or_else(|error| panic!("identify bound executable: {error:?}"));
        let bound = super::bind_executable(executable, identity)
            .await
            .unwrap_or_else(|error| panic!("bind executable: {error:?}"));
        let replacement = directory.path().join("replacement-provider");
        std::fs::write(&replacement, b"#!/bin/sh\nprintf 'replacement-bytes'")
            .unwrap_or_else(|error| panic!("write replacement executable: {error}"));
        let mut permissions = std::fs::metadata(&replacement)
            .unwrap_or_else(|error| panic!("read replacement mode: {error}"))
            .permissions();
        permissions.set_mode(0o700);
        std::fs::set_permissions(&replacement, permissions)
            .unwrap_or_else(|error| panic!("set replacement mode: {error}"));
        std::fs::rename(&replacement, directory.path().join("bound-provider"))
            .unwrap_or_else(|error| panic!("atomically replace selected executable: {error}"));
        let mut command = tokio::process::Command::new(bound.launch_path());
        #[cfg(not(target_os = "macos"))]
        bound
            .configure_command(&mut command)
            .unwrap_or_else(|error| panic!("map executable fd: {error:?}"));
        let output = command
            .output()
            .await
            .unwrap_or_else(|error| panic!("spawn bound executable: {error}"));
        assert!(output.status.success());
        assert_eq!(output.stdout, b"verified-bytes");
    }
}
