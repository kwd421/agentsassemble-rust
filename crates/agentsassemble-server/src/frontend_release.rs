//! Startup-owned immutable frontend snapshots. No source-directory polling.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
};

use fs2::FileExt;
use sha2::{Digest, Sha256};

/// The exact files selected by this runtime, independent of later build output.
pub struct FrontendRelease {
    pub root: PathBuf,
    pub build_id: String,
}

impl FrontendRelease {
    /// Copy and verify a build before publishing it to the serving runtime.
    ///
    /// # Errors
    /// Rejects absent index, links, changing sources, corrupt releases and I/O errors.
    pub fn materialize(source: &Path, state_root: &Path) -> io::Result<Self> {
        let source = source.canonicalize()?;
        if !source.join("index.html").is_file() {
            return Err(io::Error::other("frontend build has no index.html"));
        }
        let releases = state_root.join("frontend-releases");
        if state_root.canonicalize()?.starts_with(&source) {
            return Err(io::Error::other("frontend source contains runtime state"));
        }
        fs::create_dir_all(&releases)?;
        agentsassemble_persistence::secure_private_directory(&releases)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(releases.join(".lock"))?;
        lock.try_lock_exclusive()?;
        let staging = tempfile::Builder::new()
            .prefix(".staging-")
            .tempdir_in(&releases)?;
        let files = list_files(&source)?;
        for relative in &files {
            let destination = staging.path().join(relative);
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(source.join(relative), &destination)?;
        }
        let build_id = fingerprint(staging.path())?;
        if fingerprint(&source)? != build_id {
            return Err(io::Error::other("frontend build changed during snapshot"));
        }
        let root = releases.join(&build_id);
        if root.exists() {
            if !root.symlink_metadata()?.file_type().is_dir() {
                return Err(io::Error::other(
                    "retained frontend release is not a directory",
                ));
            }
            if fingerprint(&root)? != build_id {
                return Err(io::Error::other("retained frontend release is corrupt"));
            }
        } else {
            fs::rename(staging.path(), &root)?;
        }
        Ok(Self { root, build_id })
    }
}

fn list_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_files(root, Path::new(""), &mut files)?;
    files.sort();
    Ok(files)
}

fn collect_files(root: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    for entry in fs::read_dir(root.join(relative))? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let child = relative.join(entry.file_name());
        if kind.is_dir() {
            collect_files(root, &child, files)?;
        } else if kind.is_file() {
            files.push(child);
        } else {
            return Err(io::Error::other(
                "frontend build contains a link or special file",
            ));
        }
    }
    Ok(())
}

fn fingerprint(root: &Path) -> io::Result<String> {
    let mut hash = Sha256::new();
    let mut buffer = [0_u8; 8 * 1024];
    for relative in list_files(root)? {
        let name = relative
            .to_str()
            .ok_or_else(|| io::Error::other("frontend path is not UTF-8"))?
            .replace('\\', "/");
        hash.update((name.len() as u64).to_le_bytes());
        hash.update(name.as_bytes());
        let mut file = File::open(root.join(relative))?;
        let length = file.metadata()?.len();
        hash.update(length.to_le_bytes());
        let mut read = 0_u64;
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            read += count as u64;
            hash.update(&buffer[..count]);
        }
        if read != length {
            return Err(io::Error::other("frontend file changed while reading"));
        }
    }
    Ok(format!("{:x}", hash.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_replacement_retains_release_and_corruption_is_rejected() -> io::Result<()> {
        let source = tempfile::tempdir()?;
        let state = tempfile::tempdir()?;
        fs::write(source.path().join("index.html"), "first")?;
        let first = FrontendRelease::materialize(source.path(), state.path())?;
        let same = FrontendRelease::materialize(source.path(), state.path())?;
        assert_eq!(first.build_id, same.build_id);
        fs::write(source.path().join("index.html"), "second")?;
        let second = FrontendRelease::materialize(source.path(), state.path())?;
        assert_ne!(first.build_id, second.build_id);
        assert_eq!(fs::read_to_string(first.root.join("index.html"))?, "first");
        fs::write(second.root.join("index.html"), "corrupt")?;
        assert!(FrontendRelease::materialize(source.path(), state.path()).is_err());
        Ok(())
    }
}
