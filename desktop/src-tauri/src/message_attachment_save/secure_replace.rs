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
    let staging = match directory.open_dir_nofollow(&staging_name) {
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
        temporary.sync_all()?;
        staging.rename("payload", directory, filename)
    })();
    let _ = staging.remove_file("payload");
    let _ = staging.remove_open_dir();
    result
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

fn invalid_path(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
