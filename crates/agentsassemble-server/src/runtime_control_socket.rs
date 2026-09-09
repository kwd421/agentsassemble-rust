//! Same-OS-user runtime control with exact socket-file cleanup and inherited listener custody.
use std::{
    fs, io,
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt},
        net::UnixListener,
    },
    path::{Path, PathBuf},
    time::Duration,
};

use sha2::{Digest, Sha256};

pub struct RuntimeControlSocket {
    listener: UnixListener,
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl RuntimeControlSocket {
    /// Binds only after the caller has acquired the database's exclusive writer lease.
    ///
    /// # Errors
    /// Rejects insecure directories, active/unknown endpoints, and bind or filesystem failures.
    pub async fn bind(database: &Path) -> io::Result<Self> {
        let path = socket_path(database)?;
        let parent = path.parent().ok_or_else(invalid)?;
        match fs::DirBuilder::new().mode(0o700).create(parent) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        validate_directory(parent)?;
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                validate_socket(&metadata)?;
                let probe = tokio::time::timeout(
                    Duration::from_secs(1),
                    tokio::net::UnixStream::connect(&path),
                )
                .await;
                if !matches!(probe, Ok(Err(ref error)) if matches!(error.kind(), io::ErrorKind::ConnectionRefused | io::ErrorKind::NotFound))
                {
                    return Err(io::Error::new(
                        io::ErrorKind::AddrInUse,
                        "runtime control endpoint is active or unresolved",
                    ));
                }
                remove_exact(&path, metadata.dev(), metadata.ino())?;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let listener = UnixListener::bind(&path)?;
        listener.set_nonblocking(true)?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
        let metadata = fs::symlink_metadata(&path)?;
        Ok(Self {
            listener,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    /// Reclaims only the exact bound path handed off by the prior executable.
    ///
    /// # Errors
    /// Rejects a changed inode, address, owner or mode instead of binding a new endpoint.
    pub fn inherit(listener: UnixListener, database: &Path, identity: &str) -> io::Result<Self> {
        let path = socket_path(database)?;
        validate_directory(path.parent().ok_or_else(invalid)?)?;
        let metadata = fs::symlink_metadata(&path)?;
        validate_socket(&metadata)?;
        if identity != format!("{}:{}", metadata.dev(), metadata.ino())
            || listener.local_addr()?.as_pathname() != Some(path.as_path())
        {
            return Err(invalid());
        }
        listener.set_nonblocking(true)?;
        Ok(Self {
            listener,
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    #[must_use]
    pub fn identity(&self) -> String {
        format!("{}:{}", self.device, self.inode)
    }

    #[must_use]
    pub fn listener(&self) -> &UnixListener {
        &self.listener
    }

    /// Removes only this listener's socket file; never an unrelated replacement.
    ///
    /// # Errors
    /// Returns filesystem failures or changed socket-file custody.
    pub fn close(self) -> io::Result<()> {
        remove_exact(&self.path, self.device, self.inode)
    }
}

impl Drop for RuntimeControlSocket {
    fn drop(&mut self) {
        if let Err(error) = remove_exact(&self.path, self.device, self.inode) {
            tracing::error!(%error, "remove owned runtime control socket");
        }
    }
}

pub(crate) fn socket_path(database: &Path) -> io::Result<PathBuf> {
    // Unix socket paths are much shorter than valid database paths on macOS. The
    // private per-user directory keeps the complete digest within sockaddr_un.
    let database = database.canonicalize()?;
    let name = format!("{:x}.sock", Sha256::digest(database.as_os_str().as_bytes()));
    Ok(PathBuf::from(format!(
        "/tmp/agentsassemble-{}",
        rustix::process::geteuid().as_raw()
    ))
    .join(name))
}

pub(crate) fn validate_endpoint(path: &Path) -> io::Result<()> {
    validate_directory(path.parent().ok_or_else(invalid)?)?;
    validate_socket(&fs::symlink_metadata(path)?)
}

fn validate_directory(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid());
    }
    Ok(())
}

fn validate_socket(metadata: &fs::Metadata) -> io::Result<()> {
    if !metadata.file_type().is_socket()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o077 != 0
    {
        return Err(invalid());
    }
    Ok(())
}

fn remove_exact(path: &Path, device: u64, inode: u64) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.dev() == device && metadata.ino() == inode => {
            fs::remove_file(path)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(invalid()),
    }
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "runtime control socket custody is invalid",
    )
}
