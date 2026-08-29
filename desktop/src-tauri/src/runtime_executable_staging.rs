use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

use fs2::FileExt;

const ROOT_NAME: &str = "agentsassemble-desktop-executable-staging-v1";
const ROOT_LOCK_NAME: &str = ".root.lock";
const LEASE_NAME: &str = ".lease";
const DIRECTORY_PREFIX: &str = "exec-";
const MAX_STAGING_DIRECTORIES: usize = 1_024;

pub(super) struct RuntimeExecutableStaging {
    directory: tempfile::TempDir,
    _lease: File,
}

impl RuntimeExecutableStaging {
    pub(super) fn create() -> io::Result<Self> {
        Self::create_in(&std::env::temp_dir().join(ROOT_NAME))
    }

    fn create_in(root: &Path) -> io::Result<Self> {
        ensure_private_directory(root)?;
        let root_lock = acquire_lock(&root.join(ROOT_LOCK_NAME), true, true)?;
        reclaim_stale_directories(root)?;
        let directory = tempfile::Builder::new()
            .prefix(DIRECTORY_PREFIX)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(root)?;
        let lease = acquire_lock(&directory.path().join(LEASE_NAME), true, true)?;
        drop(root_lock);
        Ok(Self {
            directory,
            _lease: lease,
        })
    }

    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
}

impl Drop for RuntimeExecutableStaging {
    fn drop(&mut self) {
        let Some(root) = self.directory.path().parent() else {
            return;
        };
        let Ok(root_lock) = acquire_lock(&root.join(ROOT_LOCK_NAME), false, false) else {
            return;
        };
        let _ = reclaim_stale_directories(root);
        drop(root_lock);
    }
}

fn ensure_private_directory(path: &Path) -> io::Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "runtime staging root is not a private owned directory",
        ));
    }
    Ok(())
}

fn acquire_lock(path: &Path, create: bool, wait: bool) -> io::Result<File> {
    let file = OpenOptions::new()
        .create(create)
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(nix::libc::O_NOFOLLOW)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "runtime staging lock is not a private owned file",
        ));
    }
    if wait {
        FileExt::lock_exclusive(&file)?;
    } else {
        FileExt::try_lock_exclusive(&file)?;
    }
    Ok(file)
}

fn reclaim_stale_directories(root: &Path) -> io::Result<()> {
    let mut candidates = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if entry.file_name() == ROOT_LOCK_NAME {
            continue;
        }
        if candidates.len() == MAX_STAGING_DIRECTORIES {
            return Err(io::Error::other(
                "runtime staging directory limit was exceeded",
            ));
        }
        candidates.push(entry.path());
    }
    for path in candidates {
        reclaim_stale_directory(&path)?;
    }
    Ok(())
}

fn reclaim_stale_directory(path: &Path) -> io::Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| io::Error::other("runtime staging name is invalid"))?;
    if !name.starts_with(DIRECTORY_PREFIX) {
        return Err(io::Error::other(
            "runtime staging root contains an unknown entry",
        ));
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir()
        || metadata.uid() != nix::unistd::geteuid().as_raw()
        || metadata.permissions().mode() & 0o077 != 0
    {
        return Err(io::Error::other(
            "runtime staging entry is not a private owned directory",
        ));
    }
    let lease_path = path.join(LEASE_NAME);
    let lease = match acquire_lock(&lease_path, false, false) {
        Ok(lease) => lease,
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::remove_dir_all(path)?;
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    fs::remove_dir_all(path)?;
    drop(lease);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_owner_reclaims_only_unlocked_runtime_staging() {
        let base = tempfile::tempdir()
            .unwrap_or_else(|error| panic!("create runtime staging test root: {error}"));
        let root = base.path().join("staging");
        let active = RuntimeExecutableStaging::create_in(&root)
            .unwrap_or_else(|error| panic!("create active runtime staging: {error}"));
        let active_path = active.path().to_path_buf();

        let root_lock = acquire_lock(&root.join(ROOT_LOCK_NAME), false, true)
            .unwrap_or_else(|error| panic!("lock runtime staging root: {error}"));
        let abandoned = tempfile::Builder::new()
            .prefix(DIRECTORY_PREFIX)
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&root)
            .unwrap_or_else(|error| panic!("create abandoned runtime staging: {error}"));
        let abandoned_path = abandoned.keep();
        drop(root_lock);

        let replacement = RuntimeExecutableStaging::create_in(&root)
            .unwrap_or_else(|error| panic!("create replacement runtime staging: {error}"));
        assert!(active_path.is_dir());
        assert!(!abandoned_path.exists());
        assert!(replacement.path().is_dir());
    }
}
