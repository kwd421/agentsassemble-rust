use std::{io, path::Path};

use super::{BoundExecutable, FilesystemFailure};

pub(crate) async fn cursor_executable_identity(path: String) -> Result<String, FilesystemFailure> {
    super::run_bounded(move || identity(Path::new(&path))).await
}

pub(crate) async fn bind_cursor_executable(
    path: String,
    expected_identity: String,
) -> Result<BoundExecutable, FilesystemFailure> {
    super::run_bounded(move || {
        let path = Path::new(&path);
        #[cfg(unix)]
        if is_script(path)? {
            return package::bind(path, &expected_identity);
        }
        let mut bound = super::bind_executable_sync(path, &expected_identity)?;
        bound.allows_child_processes = true;
        Ok(bound)
    })
    .await
}

fn identity(path: &Path) -> io::Result<String> {
    #[cfg(unix)]
    if is_script(path)? {
        return package::manifest(path).map(|manifest| manifest.identity());
    }
    super::executable_identity_sync(path)
}

// Native distributions retain the single native executable contract. Unix script
// distributions must provide the complete sibling package; missing dependencies
// never select the native path.
#[cfg(unix)]
fn is_script(path: &Path) -> io::Result<bool> {
    use std::io::Read;
    let mut prefix = [0_u8; 2];
    std::fs::File::open(path)?.read_exact(&mut prefix)?;
    Ok(prefix == *b"#!")
}

#[cfg(unix)]
mod package {
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Read},
        os::unix::fs::{OpenOptionsExt, PermissionsExt},
        path::{Path, PathBuf},
    };

    use agentsassemble_domain::{stable_bundle_identity, stable_content_identity};
    use same_file::Handle;
    use walkdir::WalkDir;

    use super::super::{BoundExecutable, ExecutableStaging};

    // The observed package is 234 MB / 459 files. Bounds constrain traversal and
    // copying of user-selected executable directories, including damaged installs.
    const MAX_ENTRIES: usize = 4_096;
    const MAX_BYTES: u64 = 512 * 1024 * 1024;
    const MAX_DEPTH: usize = 32;

    struct Member {
        relative: String,
        identity: String,
        bytes: u64,
        executable: bool,
    }

    pub(super) struct Manifest {
        root: PathBuf,
        entry: String,
        members: Vec<Member>,
    }

    impl Manifest {
        pub(super) fn identity(&self) -> String {
            let mut values = self
                .members
                .iter()
                .flat_map(|member| {
                    [
                        member.relative.as_str(),
                        member.identity.as_str(),
                        if member.executable {
                            "executable"
                        } else {
                            "data"
                        },
                    ]
                })
                .collect::<Vec<_>>();
            values.insert(0, &self.entry);
            stable_bundle_identity("cursor-unix-package-v1", &values)
        }
    }

    fn open_member(path: &Path) -> io::Result<File> {
        if path.canonicalize()? != path {
            return Err(io::Error::other("Cursor package path is not canonical"));
        }
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK)
            .open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other(
                "Cursor package member is not a regular file",
            ));
        }
        Ok(file)
    }

    pub(super) fn manifest(path: &Path) -> io::Result<Manifest> {
        if path.canonicalize()? != path || !super::super::is_executable_file(path)? {
            return Err(io::Error::other(
                "Cursor executable authority is not canonical",
            ));
        }
        let root = path
            .parent()
            .ok_or_else(|| io::Error::other("Cursor package missing"))?;
        let entry = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| io::Error::other("Cursor entry name is invalid"))?
            .to_owned();
        let mut members = Vec::new();
        let mut total_bytes = 0_u64;
        for (count, item) in WalkDir::new(root)
            .follow_links(false)
            .max_depth(MAX_DEPTH)
            .into_iter()
            .enumerate()
        {
            let item = item.map_err(io::Error::other)?;
            if count >= MAX_ENTRIES || (item.depth() == MAX_DEPTH && item.file_type().is_dir()) {
                return Err(io::Error::other("Cursor package traversal bound exceeded"));
            }
            if item.file_type().is_dir() {
                continue;
            }
            if !item.file_type().is_file() {
                return Err(io::Error::other(
                    "Cursor package contains a non-regular member",
                ));
            }
            let mut file = open_member(item.path())?;
            let metadata = file.metadata()?;
            total_bytes = total_bytes
                .checked_add(metadata.len())
                .filter(|total| *total <= MAX_BYTES)
                .ok_or_else(|| io::Error::other("Cursor package byte bound exceeded"))?;
            let handle = Handle::from_file(file.try_clone()?)?;
            let identity = stable_content_identity(&handle, (&mut file).take(metadata.len() + 1))?;
            if file.metadata()?.len() != metadata.len() {
                return Err(io::Error::other("Cursor package changed during discovery"));
            }
            let relative = item
                .path()
                .strip_prefix(root)
                .map_err(io::Error::other)?
                .to_str()
                .ok_or_else(|| io::Error::other("Cursor package path is not UTF-8"))?
                .to_owned();
            members.push(Member {
                relative,
                identity,
                bytes: metadata.len(),
                executable: metadata.permissions().mode() & 0o111 != 0,
            });
        }
        members.sort_by(|left, right| left.relative.cmp(&right.relative));
        if !members
            .iter()
            .any(|member| member.relative == "node" && member.executable)
            || !members.iter().any(|member| member.relative == "index.js")
        {
            return Err(io::Error::other(
                "Cursor package Node or entry module is missing",
            ));
        }
        Ok(Manifest {
            root: root.to_owned(),
            entry,
            members,
        })
    }

    pub(super) fn bind(path: &Path, expected: &str) -> io::Result<BoundExecutable> {
        let manifest = manifest(path)?;
        if manifest.identity() != expected {
            return Err(io::Error::other("Cursor package identity changed"));
        }
        let staging = ExecutableStaging::create()?;
        let destination = staging.path().join("package");
        fs::create_dir(&destination)?;
        for member in &manifest.members {
            let mut source = open_member(&manifest.root.join(&member.relative))?;
            let handle = Handle::from_file(source.try_clone()?)?;
            let target = destination.join(&member.relative);
            fs::create_dir_all(
                target
                    .parent()
                    .ok_or_else(|| io::Error::other("Cursor member parent missing"))?,
            )?;
            let mut staged = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(&target)?;
            if io::copy(&mut (&mut source).take(member.bytes + 1), &mut staged)? != member.bytes {
                return Err(io::Error::other(
                    "Cursor package size changed during staging",
                ));
            }
            staged.sync_all()?;
            super::super::verify_staged_identity(&handle, &member.identity, &mut staged)?;
            fs::set_permissions(
                &target,
                fs::Permissions::from_mode(if member.executable { 0o500 } else { 0o400 }),
            )?;
        }
        let launch_path = destination.join(manifest.entry);
        let file = File::open(&launch_path)?;
        Ok(BoundExecutable {
            file,
            launch_path: launch_path
                .to_str()
                .ok_or_else(|| io::Error::other("Cursor staged path is not UTF-8"))?
                .to_owned(),
            companion_files: Vec::new(),
            allows_child_processes: true,
            _staging: Some(staging),
            #[cfg(any(target_os = "linux", target_os = "android"))]
            inherited_executable_fd: false,
        })
    }
}

#[cfg(all(test, unix))]
#[path = "cursor_executable_tests.rs"]
mod tests;
