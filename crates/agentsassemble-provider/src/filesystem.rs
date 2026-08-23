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

#[path = "codex_executable.rs"]
mod codex_executable;

pub(crate) use codex_executable::{
    bind_codex_executable, codex_executable_identity, resolve_codex_executable,
};

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
    file: File,
    launch_path: String,
    companion_files: Vec<File>,
    allows_child_processes: bool,
    #[cfg(unix)]
    _staging: Option<tempfile::TempDir>,
    #[cfg(any(target_os = "linux", target_os = "android"))]
    inherited_executable_fd: bool,
}

impl BoundExecutable {
    pub(crate) fn launch_path(&self) -> &str {
        let _ = &self.file;
        let _ = &self.companion_files;
        &self.launch_path
    }

    #[cfg(unix)]
    pub(crate) fn requires_inherited_executable_fd(&self) -> bool {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        return self.inherited_executable_fd;
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        {
            let _ = &self.file;
            false
        }
    }

    #[cfg(unix)]
    pub(crate) const fn allows_child_processes(&self) -> bool {
        self.allows_child_processes
    }

    pub(crate) fn stage_private_companion(&self, name: &str) -> io::Result<PrivateExecutable> {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;

        if name.is_empty() || name.contains(['/', '\\', '\0']) {
            return Err(io::Error::other("companion executable name is invalid"));
        }
        let mut staging_builder = tempfile::Builder::new();
        staging_builder.prefix("agentsassemble-companion-");
        #[cfg(unix)]
        staging_builder.permissions(std::fs::Permissions::from_mode(0o700));
        let staging = staging_builder.tempdir()?;
        let staged_path = staging.path().join(name);
        let mut source = self.file.try_clone()?;
        source.rewind()?;
        let source_handle = Handle::from_file(source.try_clone()?)?;
        let expected_identity = stable_content_identity(&source_handle, &mut source)?;
        source.rewind()?;
        let mut staged = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&staged_path)?;
        io::copy(&mut source, &mut staged)?;
        staged.sync_all()?;
        verify_staged_identity(&source_handle, &expected_identity, &mut staged)?;
        #[cfg(unix)]
        std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o500))?;
        drop(staged);
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

            options.share_mode(FILE_SHARE_READ);
        }
        let file = options.open(&staged_path)?;
        Ok(PrivateExecutable {
            file,
            path: staged_path,
            staging,
        })
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn try_clone_file(&self) -> io::Result<File> {
        self.file.try_clone()
    }

    #[cfg(all(test, any(target_os = "linux", target_os = "android")))]
    pub(crate) fn configure_command(
        &self,
        command: &mut tokio::process::Command,
    ) -> Result<(), FilesystemFailure> {
        use command_fds::{CommandFdExt, FdMapping};

        let file = self
            .file
            .try_clone()
            .map_err(|_| FilesystemFailure::Failed)?;
        command
            .fd_mappings(vec![FdMapping {
                parent_fd: file.into(),
                child_fd: 3,
            }])
            .map(|_| ())
            .map_err(|_| FilesystemFailure::Failed)
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn configure_std_command(
        &self,
        command: &mut std::process::Command,
    ) -> io::Result<()> {
        self.configure_std_command_with_mappings(command, Vec::new())
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    pub(crate) fn configure_std_command_with_mappings(
        &self,
        command: &mut std::process::Command,
        mut mappings: Vec<command_fds::FdMapping>,
    ) -> io::Result<()> {
        use command_fds::{CommandFdExt, FdMapping};

        mappings.push(FdMapping {
            parent_fd: self.file.try_clone()?.into(),
            child_fd: 3,
        });
        command
            .fd_mappings(mappings)
            .map(|_| ())
            .map_err(io::Error::other)
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    pub(crate) fn configure_std_command(
        &self,
        command: &mut std::process::Command,
    ) -> io::Result<()> {
        self.configure_std_command_with_mappings(command, Vec::new())
    }

    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    pub(crate) fn configure_std_command_with_mappings(
        &self,
        command: &mut std::process::Command,
        mappings: Vec<command_fds::FdMapping>,
    ) -> io::Result<()> {
        use command_fds::CommandFdExt;

        drop(self.file.try_clone()?);
        if !mappings.is_empty() {
            command.fd_mappings(mappings).map_err(io::Error::other)?;
        }
        Ok(())
    }
}

pub(crate) struct PrivateExecutable {
    file: File,
    path: PathBuf,
    staging: tempfile::TempDir,
}

impl PrivateExecutable {
    pub(crate) fn path(&self) -> &Path {
        let _ = &self.file;
        &self.path
    }

    pub(crate) fn directory(&self) -> &Path {
        self.staging.path()
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

pub(crate) async fn runtime_executable_identity(
    provider_kind: &str,
    path: String,
) -> Result<String, FilesystemFailure> {
    if provider_kind == "codex_live_session" {
        codex_executable_identity(path).await
    } else {
        executable_identity(path).await
    }
}

pub(crate) async fn bind_executable(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    run_bounded(move || bind_executable_sync(Path::new(&path), &expected_identity)).await
}

pub(crate) async fn bind_executable_with_children(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    let mut executable = bind_executable(path, expected_identity).await?;
    executable.allows_child_processes = true;
    Ok(executable)
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
    #[cfg(unix)]
    return bind_verified_unix_executable(file, &handle, expected_identity);
    #[cfg(not(unix))]
    let launch_path = canonical
        .to_str()
        .ok_or_else(|| io::Error::other("executable path is not UTF-8"))?
        .to_owned();
    #[cfg(not(unix))]
    Ok(BoundExecutable {
        file,
        launch_path,
        companion_files: Vec::new(),
        allows_child_processes: false,
    })
}

pub(crate) fn bind_helper_executable_sync(path: &Path) -> io::Result<BoundExecutable> {
    #[cfg(windows)]
    {
        let canonical = path.canonicalize()?;
        let identity = executable_identity_sync(&canonical)?;
        return bind_executable_sync(&canonical, &identity);
    }
    #[cfg(unix)]
    {
        if !is_executable_file(path)? {
            return Err(io::Error::other("helper executable is not executable"));
        }
        let mut file = File::open(path)?;
        let handle = Handle::from_file(file.try_clone()?)?;
        let expected_identity = stable_content_identity(&handle, &mut file)?;
        file.rewind()?;
        bind_verified_unix_executable(file, &handle, &expected_identity)
    }
}

#[cfg(windows)]
pub(crate) fn bind_current_helper_executable() -> io::Result<BoundExecutable> {
    bind_helper_executable_sync(&std::env::current_exe()?)
}

#[cfg(unix)]
fn bind_verified_unix_executable(
    mut file: File,
    handle: &Handle,
    expected_identity: &str,
) -> io::Result<BoundExecutable> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let file = stage_sealed_executable(&mut file, handle, expected_identity)?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let launch_path = "/proc/self/fd/3".to_owned();
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
    let (file, launch_path, staging) =
        stage_private_executable(&mut file, handle, expected_identity)?;
    Ok(BoundExecutable {
        file,
        launch_path,
        companion_files: Vec::new(),
        allows_child_processes: false,
        #[cfg(any(target_os = "linux", target_os = "android"))]
        _staging: None,
        #[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
        _staging: Some(staging),
        #[cfg(any(target_os = "linux", target_os = "android"))]
        inherited_executable_fd: true,
    })
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn stage_sealed_executable(
    source: &mut File,
    source_handle: &Handle,
    expected_identity: &str,
) -> io::Result<File> {
    use rustix::fs::{MemfdFlags, Mode, SealFlags};

    let mut staged = match rustix::fs::memfd_create(
        "agentsassemble-provider",
        MemfdFlags::ALLOW_SEALING | MemfdFlags::EXEC,
    ) {
        Ok(file) => File::from(file),
        Err(rustix::io::Errno::INVAL) => File::from(rustix::fs::memfd_create(
            "agentsassemble-provider",
            MemfdFlags::ALLOW_SEALING,
        )?),
        Err(error) => return Err(error.into()),
    };
    rustix::fs::fchmod(&staged, Mode::RUSR | Mode::XUSR)?;
    copy_and_verify_staged(source, source_handle, expected_identity, &mut staged)?;
    let required = SealFlags::SEAL | SealFlags::SHRINK | SealFlags::GROW | SealFlags::WRITE;
    rustix::fs::fcntl_add_seals(&staged, required)?;
    if !rustix::fs::fcntl_get_seals(&staged)?.contains(required) {
        return Err(io::Error::other("staged executable seals are incomplete"));
    }
    staged.rewind()?;
    Ok(staged)
}

#[cfg(unix)]
fn stage_private_executable(
    source: &mut File,
    source_handle: &Handle,
    expected_identity: &str,
) -> io::Result<(File, String, tempfile::TempDir)> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    let staging = tempfile::Builder::new()
        .prefix("agentsassemble-provider-exec-")
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()?;
    let staging_metadata = std::fs::symlink_metadata(staging.path())?;
    if !staging_metadata.is_dir()
        || staging_metadata.uid() != rustix::process::geteuid().as_raw()
        || staging_metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "executable staging directory is not private",
        ));
    }
    let staged_path = staging.path().join("provider");
    let mut staged = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&staged_path)?;
    copy_and_verify_staged(source, source_handle, expected_identity, &mut staged)?;
    std::fs::set_permissions(&staged_path, std::fs::Permissions::from_mode(0o500))?;
    let launch_path = staged_path
        .to_str()
        .ok_or_else(|| io::Error::other("staged executable path is not UTF-8"))?
        .to_owned();
    Ok((staged, launch_path, staging))
}

#[cfg(unix)]
fn copy_and_verify_staged(
    source: &mut File,
    source_handle: &Handle,
    expected_identity: &str,
    staged: &mut File,
) -> io::Result<()> {
    source.rewind()?;
    io::copy(source, &mut *staged)?;
    staged.sync_all()?;
    verify_staged_identity(source_handle, expected_identity, staged)
}

#[cfg(any(unix, windows))]
fn verify_staged_identity(
    source_handle: &Handle,
    expected_identity: &str,
    staged: &mut File,
) -> io::Result<()> {
    staged.rewind()?;
    if stable_content_identity(source_handle, &mut *staged)? != expected_identity {
        return Err(io::Error::other(
            "staged executable identity does not match verified authority",
        ));
    }
    Ok(())
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

    #[cfg(unix)]
    use agentsassemble_domain::stable_content_identity;
    #[cfg(unix)]
    use same_file::Handle;

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
    #[test]
    fn staged_bytes_are_compared_directly_to_the_persisted_source_identity() {
        use std::io::{Seek, Write};

        let mut source = tempfile::tempfile()
            .unwrap_or_else(|error| panic!("create staged identity source: {error}"));
        source
            .write_all(b"verified helper bytes")
            .unwrap_or_else(|error| panic!("write staged identity source: {error}"));
        source
            .rewind()
            .unwrap_or_else(|error| panic!("rewind staged identity source: {error}"));
        let handle = Handle::from_file(
            source
                .try_clone()
                .unwrap_or_else(|error| panic!("clone staged identity source: {error}")),
        )
        .unwrap_or_else(|error| panic!("identify staged identity source: {error}"));
        let expected = stable_content_identity(&handle, &mut source)
            .unwrap_or_else(|error| panic!("hash staged identity source: {error}"));
        let mut staged = tempfile::tempfile()
            .unwrap_or_else(|error| panic!("create staged identity target: {error}"));
        staged
            .write_all(b"different bytes from a later source read")
            .unwrap_or_else(|error| panic!("write staged identity target: {error}"));
        assert!(super::verify_staged_identity(&handle, &expected, &mut staged).is_err());
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
        std::fs::write(
            directory.path().join("bound-provider"),
            b"#!/bin/sh\nprintf 'in-place-replacement'",
        )
        .unwrap_or_else(|error| panic!("overwrite selected inode in place: {error}"));
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
        #[cfg(any(target_os = "linux", target_os = "android"))]
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
