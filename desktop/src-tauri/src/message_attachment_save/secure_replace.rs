use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

use cap_fs_ext::DirExt;
use cap_std::{ambient_authority, fs::Dir};
use uuid::Uuid;

pub(super) fn save_atomically(path: &Path, content: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| invalid_path("save path has no parent"))?;
    let filename = path
        .file_name()
        .filter(|filename| !filename.is_empty())
        .ok_or_else(|| invalid_path("save path has no filename"))?;
    let directory = open_absolute_directory(parent)?;
    match directory.symlink_metadata(filename) {
        Ok(metadata) if !metadata.file_type().is_file() => {
            return Err(invalid_path("save target is not a regular file"));
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    replace_in_directory(&directory, filename, content)
}

fn open_absolute_directory(path: &Path) -> io::Result<Dir> {
    if !path.is_absolute() {
        return Err(invalid_path("save parent is not absolute"));
    }
    let mut root = PathBuf::new();
    let mut names = Vec::<OsString>::new();
    let mut rooted = false;
    for component in path.components() {
        match component {
            Component::Prefix(prefix) if !rooted && names.is_empty() => {
                root.push(prefix.as_os_str());
            }
            Component::RootDir if !rooted && names.is_empty() => {
                root.push(component.as_os_str());
                rooted = true;
            }
            Component::Normal(name) if rooted => names.push(name.to_owned()),
            Component::CurDir if rooted => {}
            _ => return Err(invalid_path("save parent has an invalid component")),
        }
    }
    if !rooted {
        return Err(invalid_path("save parent has no filesystem root"));
    }
    let mut directory = Dir::open_ambient_dir(root, ambient_authority())?;
    for name in names {
        directory = directory.open_dir_nofollow(name)?;
    }
    Ok(directory)
}

fn replace_in_directory(directory: &Dir, filename: &OsStr, content: &[u8]) -> io::Result<()> {
    use cap_std::fs::{DirBuilder, OpenOptions};

    let staging_name = format!(".agentsassemble-save-{}", Uuid::new_v4().simple());
    let mut builder = DirBuilder::new();
    configure_private_directory(&mut builder);
    directory.create_dir_with(&staging_name, &builder)?;
    let staging = match open_staging_directory(directory, &staging_name) {
        Ok(staging) => staging,
        Err(error) => {
            let _ = directory.remove_dir(&staging_name);
            return Err(error);
        }
    };
    if let Err(error) = secure_staging_directory(&staging) {
        let _ = staging.remove_open_dir();
        return Err(error);
    }

    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        configure_private_file(&mut options);
        let mut temporary = staging.open_with("payload", &options)?;
        #[cfg(windows)]
        secure_staging_file(&temporary)?;
        temporary.write_all(content)?;
        apply_download_origin(&staging, &temporary)?;
        temporary.sync_all()?;
        staging.rename("payload", directory, filename)
    })();
    let _ = staging.remove_file("payload");
    let _ = staging.remove_open_dir();
    result
}

#[cfg(not(windows))]
fn open_staging_directory(directory: &Dir, name: &str) -> io::Result<Dir> {
    directory.open_dir_nofollow(name)
}

#[cfg(windows)]
fn open_staging_directory(directory: &Dir, name: &str) -> io::Result<Dir> {
    use cap_fs_ext::{FollowSymlinks, OpenOptionsFollowExt, OpenOptionsMaybeDirExt};
    use cap_std::fs::{OpenOptions, OpenOptionsExt};
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

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

#[cfg(unix)]
fn configure_private_directory(builder: &mut cap_std::fs::DirBuilder) {
    use cap_std::fs::DirBuilderExt;

    builder.mode(0o700);
}

#[cfg(windows)]
fn configure_private_directory(_builder: &mut cap_std::fs::DirBuilder) {}

#[cfg(unix)]
fn configure_private_file(options: &mut cap_std::fs::OpenOptions) {
    use cap_std::fs::OpenOptionsExt;

    options.mode(0o600);
}

#[cfg(windows)]
fn configure_private_file(_options: &mut cap_std::fs::OpenOptions) {}

#[cfg(unix)]
fn secure_staging_directory(directory: &Dir) -> io::Result<()> {
    use cap_std::fs::MetadataExt;

    let metadata = directory.dir_metadata()?;
    if metadata.uid() == nix::unistd::geteuid().as_raw() && metadata.mode().trailing_zeros() >= 6 {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "save staging directory is not private",
        ))
    }
}

#[cfg(windows)]
fn secure_staging_directory(directory: &Dir) -> io::Result<()> {
    let handle = directory.try_clone()?.into_std_file();
    crate::private_fs::secure_directory_handle(&handle)
}

#[cfg(windows)]
fn secure_staging_file(file: &cap_std::fs::File) -> io::Result<()> {
    let handle = file.try_clone()?.into_std();
    crate::private_fs::secure_file(&handle)
}

#[cfg(target_os = "macos")]
fn apply_download_origin(_directory: &Dir, file: &cap_std::fs::File) -> io::Result<()> {
    use std::time::{SystemTime, UNIX_EPOCH};
    use xattr::FileExt;

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let identifier = Uuid::new_v4().hyphenated().to_string().to_uppercase();
    let marker = format!("0083;{timestamp:x};AgentsAssemble;{identifier}");
    file.try_clone()?
        .into_std()
        .set_xattr("com.apple.quarantine", marker.as_bytes())
}

#[cfg(windows)]
fn apply_download_origin(directory: &Dir, _file: &cap_std::fs::File) -> io::Result<()> {
    use cap_std::fs::OpenOptions;

    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    let mut marker = directory.open_with("payload:Zone.Identifier", &options)?;
    marker.write_all(b"[ZoneTransfer]\r\nZoneId=3\r\n")?;
    marker.sync_all()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn apply_download_origin(_directory: &Dir, _file: &cap_std::fs::File) -> io::Result<()> {
    Ok(())
}

fn invalid_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

#[cfg(all(test, windows))]
mod windows_tests {
    use std::fs;

    use cap_fs_ext::DirExt;
    use tempfile::tempdir;

    use super::{open_absolute_directory, open_staging_directory};

    #[test]
    fn staging_open_fails_closed_around_an_existing_or_later_handle() {
        let root = tempdir().unwrap_or_else(|error| panic!("create save directory: {error}"));
        let root = fs::canonicalize(root.path())
            .unwrap_or_else(|error| panic!("resolve save directory: {error}"));
        let staging_path = root.join("staging");
        fs::create_dir(&staging_path)
            .unwrap_or_else(|error| panic!("create staging directory: {error}"));
        let directory = open_absolute_directory(&root)
            .unwrap_or_else(|error| panic!("open save directory: {error}"));

        let existing = directory
            .open_dir_nofollow("staging")
            .unwrap_or_else(|error| panic!("open competing handle: {error}"));
        assert!(open_staging_directory(&directory, "staging").is_err());
        drop(existing);

        let exclusive = open_staging_directory(&directory, "staging")
            .unwrap_or_else(|error| panic!("open exclusive staging directory: {error}"));
        assert!(directory.open_dir_nofollow("staging").is_err());
        drop(exclusive);
        directory
            .open_dir_nofollow("staging")
            .unwrap_or_else(|error| panic!("reopen released staging directory: {error}"));
    }
}
