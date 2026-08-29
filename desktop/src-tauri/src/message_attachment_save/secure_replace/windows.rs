use std::{io, path::Path};

use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
use cap_std::fs::{Dir, OpenOptions, OpenOptionsExt};
use same_file::Handle;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

use super::invalid_path;

pub(super) fn create_staging_directory(
    directory: &Dir,
    parent: &Path,
    name: &str,
) -> io::Result<Dir> {
    verify_parent_identity(directory, parent)?;
    crate::private_fs::create_private_directory(&parent.join(name))?;
    let staging = open_exclusive(directory, name)?;
    let handle = staging.try_clone()?.into_std_file();
    crate::private_fs::validate_private_directory_handle(&handle)?;
    Ok(staging)
}

fn verify_parent_identity(directory: &Dir, parent: &Path) -> io::Result<()> {
    let retained = Handle::from_file(directory.try_clone()?.into_std_file())?;
    let named = Handle::from_path(parent)?;
    if retained == named {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "save parent identity changed",
        ))
    }
}

fn open_exclusive(directory: &Dir, name: &str) -> io::Result<Dir> {
    let mut options = OpenOptions::new();
    options.read(true);
    options.maybe_dir(true);
    options.follow(FollowSymlinks::No);
    options.share_mode(0);
    options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    let handle = directory.open_with(name, &options)?;
    let metadata = handle.metadata()?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid_path("save staging entry is not a directory"));
    }
    Ok(Dir::from_std_file(handle.into_std()))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use cap_fs_ext::DirExt;
    use tempfile::tempdir;

    use super::{create_staging_directory, open_exclusive};
    use crate::message_attachment_save::secure_replace::open_absolute_directory;

    #[test]
    fn private_staging_directory_is_created_and_retained() {
        let root = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let root = fs::canonicalize(root.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        let directory = open_absolute_directory(&root)
            .unwrap_or_else(|error| panic!("open save directory: {error}"));

        let staging = create_staging_directory(&directory, &root, "private")
            .unwrap_or_else(|error| panic!("create private staging directory: {error}"));
        assert!(directory.open_dir_nofollow("private").is_err());
        staging
            .remove_open_dir()
            .unwrap_or_else(|error| panic!("remove private staging directory: {error}"));
    }

    #[test]
    fn exclusive_open_rejects_an_existing_or_later_handle() {
        let root = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let root = fs::canonicalize(root.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        fs::create_dir(root.join("staging"))
            .unwrap_or_else(|error| panic!("create staging directory: {error}"));
        let directory = open_absolute_directory(&root)
            .unwrap_or_else(|error| panic!("open save directory: {error}"));

        let existing = directory
            .open_dir_nofollow("staging")
            .unwrap_or_else(|error| panic!("open competing handle: {error}"));
        assert!(open_exclusive(&directory, "staging").is_err());
        drop(existing);

        let exclusive = open_exclusive(&directory, "staging")
            .unwrap_or_else(|error| panic!("open exclusive staging directory: {error}"));
        assert!(directory.open_dir_nofollow("staging").is_err());
        drop(exclusive);
        directory
            .open_dir_nofollow("staging")
            .unwrap_or_else(|error| panic!("reopen released staging directory: {error}"));
    }
}
