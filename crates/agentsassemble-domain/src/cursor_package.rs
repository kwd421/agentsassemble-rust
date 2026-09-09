//! Shared Cursor package authority used at selection, persistence and runtime boundaries.
use crate::{stable_bundle_identity, stable_content_identity};
use same_file::Handle;
use std::{
    fs::File,
    io::{self, Read},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

// The observed package is 234 MB / 459 files. Bounds constrain traversal and
// copying of user-selected executable directories, including damaged installs.
const MAX_ENTRIES: usize = 4_096;
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: usize = 32;

pub struct CursorPackageMember {
    pub relative: String,
    pub identity: String,
    pub bytes: u64,
    pub executable: bool,
}

pub struct CursorExecutablePackage {
    root: PathBuf,
    pub entry: String,
    pub members: Vec<CursorPackageMember>,
}

impl CursorExecutablePackage {
    /// Inspect the canonical Cursor entry. Native entries have no sibling package.
    /// # Errors
    /// Rejects missing, non-canonical, non-regular or oversized package members.
    pub fn inspect(path: &Path) -> io::Result<Option<Self>> {
        let mut file = open_member(path)?;
        let mut prefix = [0_u8; 2];
        file.read_exact(&mut prefix)?;
        if prefix != *b"#!" {
            return Ok(None);
        }
        manifest(path).map(Some)
    }

    /// Open a discovered member for verified staging by the runtime owner.
    /// # Errors
    /// Rejects a replaced path or a non-regular member.
    pub fn open_member(&self, member: &CursorPackageMember) -> io::Result<File> {
        open_member(&self.root.join(&member.relative))
    }

    /// Exact entry, relative paths, opened file/content identities and modes.
    #[must_use]
    pub fn identity(&self) -> String {
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
    let file = File::from(rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK,
        rustix::fs::Mode::empty(),
    )?);
    if !file.metadata()?.is_file() {
        return Err(io::Error::other(
            "Cursor package member is not a regular file",
        ));
    }
    Ok(file)
}

fn manifest(path: &Path) -> io::Result<CursorExecutablePackage> {
    if path.canonicalize()? != path || !is_executable(path)? {
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
        members.push(CursorPackageMember {
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
    Ok(CursorExecutablePackage {
        root: root.to_owned(),
        entry,
        members,
    })
}

fn is_executable(path: &Path) -> io::Result<bool> {
    let metadata = open_member(path)?.metadata()?;
    Ok(metadata.permissions().mode() & 0o111 != 0)
}
