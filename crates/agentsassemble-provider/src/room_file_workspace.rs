//! Room transfers use the existing capability-bound configured workspace.
use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt, OpenOptionsSyncExt};
use cap_std::fs::{Dir, OpenOptions};
use std::{io, path::Path};
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

/// Open a regular source without reading bytes. Capability resolution confines symlinks.
/// # Errors
/// Rejects paths outside the bound workspace, special files and replaced workspaces.
pub fn open_workspace_attachment(
    workspace: &str,
    identity: &str,
    path: &str,
) -> io::Result<(std::fs::File, String, usize)> {
    let root = crate::workspace_files::bind(workspace, identity)?;
    let requested = Path::new(path);
    let relative = if requested.is_absolute() {
        requested
            .strip_prefix(workspace)
            .map_err(|_| io::Error::other("Path is outside the working folder."))?
    } else {
        requested
    };
    // One non-blocking open, with no separate resolution step before it: resolving
    // first would open the last component in blocking mode, and a named pipe there
    // would park this thread for good. The capability walk still confines `..`,
    // absolute prefixes and symlink targets to the working folder.
    let mut options = OpenOptions::new();
    options
        .read(true)
        .follow(FollowSymlinks::Yes)
        .nonblock(true);
    let file = root.open_with(relative, &options)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::other("Only regular files can be uploaded."));
    }
    let name = relative
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| io::Error::other("Invalid filename."))?
        .to_owned();
    let size =
        usize::try_from(metadata.len()).map_err(|_| io::Error::other("Invalid file size."))?;
    Ok((file.into_std(), name, size))
}

/// Owns the private temporary file until atomic publication; Drop removes partial output.
pub struct WorkspaceAttachmentSink {
    root: Dir,
    directory: Dir,
    folder: std::path::PathBuf,
    path: std::path::PathBuf,
    pub file: tokio::fs::File,
    published: bool,
}
impl WorkspaceAttachmentSink {
    /// # Errors
    /// Rejects redirected output folders or replaced workspace custody.
    pub fn new(workspace: &str, identity: &str, filename: &str) -> io::Result<Self> {
        let root = crate::workspace_files::bind(workspace, identity)?;
        let subfolder = ".agentsassemble-attachments";
        match root.create_dir(subfolder) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
        let parent = root.open_dir_nofollow(subfolder)?;
        let unique = Uuid::new_v4().to_string();
        parent.create_dir(&unique)?;
        let directory = parent.open_dir_nofollow(&unique)?;
        let mut options = OpenOptions::new();
        options
            .write(true)
            .create_new(true)
            .follow(FollowSymlinks::No);
        let file = directory.open_with("partial", &options)?.into_std();
        let folder = Path::new(subfolder).join(unique);
        // Metadata has crossed the canonical filename owner; output never uses input paths.
        let path = Path::new(workspace).join(&folder).join(filename);
        Ok(Self {
            root,
            directory,
            folder,
            path,
            file: tokio::fs::File::from_std(file),
            published: false,
        })
    }
    /// # Errors
    /// Flush/sync/rename errors retain no successful saved-path result.
    pub async fn publish(mut self) -> io::Result<String> {
        self.file.flush().await?;
        self.file.sync_all().await?;
        let filename = self
            .path
            .file_name()
            .ok_or_else(|| io::Error::other("Invalid filename."))?;
        self.directory
            .rename("partial", &self.directory, filename)?;
        self.published = true;
        self.path
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| io::Error::other("Invalid output path."))
    }
}
/// Remove a file this sink published, with its private folder, when its read is refused
/// after the fact. Only paths of the shape the sink creates are touched.
pub(crate) fn discard_saved_attachment(path: &str) {
    let path = Path::new(path);
    let Some(folder) = path.parent() else { return };
    let owned = folder
        .parent()
        .and_then(Path::file_name)
        .is_some_and(|name| name == ".agentsassemble-attachments");
    if owned && Uuid::parse_str(&folder.file_name().unwrap_or_default().to_string_lossy()).is_ok() {
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_dir(folder);
    }
}
impl Drop for WorkspaceAttachmentSink {
    fn drop(&mut self) {
        if !self.published {
            let _ = self.directory.remove_file("partial");
            let _ = self.root.remove_dir(&self.folder);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn workspace() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let identity = agentsassemble_domain::stable_identity_hash(
            &same_file::Handle::from_path(&path).unwrap(),
        );
        (dir, path, identity)
    }
    // A named pipe would block a plain open until a writer appears; the upload path
    // must refuse it at once instead of parking a server thread.
    #[cfg(unix)]
    #[test]
    fn a_named_pipe_is_refused_without_blocking() {
        let (_dir, root, identity) = workspace();
        let created = std::process::Command::new("mkfifo")
            .arg(Path::new(&root).join("pipe"))
            .status()
            .unwrap();
        assert!(created.success());
        std::os::unix::fs::symlink("pipe", Path::new(&root).join("link-to-pipe")).unwrap();
        for path in ["pipe", "link-to-pipe"] {
            let (done, result) = std::sync::mpsc::channel();
            let (root, identity) = (root.clone(), identity.clone());
            std::thread::spawn(move || {
                let _ = done.send(open_workspace_attachment(&root, &identity, path).is_err());
            });
            assert_eq!(
                result.recv_timeout(std::time::Duration::from_secs(3)),
                Ok(true),
                "{path}"
            );
        }
    }
    #[tokio::test]
    async fn source_confinement_and_atomic_sink_keep_workspace_custody() {
        let (_dir, root, identity) = workspace();
        let outside = tempfile::tempdir().unwrap();
        let external = outside.path().join("outside.bin");
        std::fs::write(&external, b"outside").unwrap();
        std::fs::write(Path::new(&root).join("inside.bin"), b"inside").unwrap();
        std::fs::create_dir(Path::new(&root).join("nested")).unwrap();
        let (file, name, size) =
            open_workspace_attachment(&root, &identity, "nested/../inside.bin").unwrap();
        assert_eq!((name.as_str(), size), ("inside.bin", 6));
        drop(file);
        for path in ["..", "nested", external.to_str().unwrap()] {
            assert!(open_workspace_attachment(&root, &identity, path).is_err());
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&external, Path::new(&root).join("escape")).unwrap();
            assert!(open_workspace_attachment(&root, &identity, "escape").is_err());
            std::os::unix::net::UnixListener::bind(Path::new(&root).join("socket")).unwrap();
            assert!(open_workspace_attachment(&root, &identity, "socket").is_err());
        }
        let mut sink = WorkspaceAttachmentSink::new(&root, &identity, "result.bin").unwrap();
        let final_path = sink.path.clone();
        sink.file.write_all(b"exact bytes").await.unwrap();
        assert!(!final_path.exists());
        let path = sink.publish().await.unwrap();
        assert_eq!(tokio::fs::read(&path).await.unwrap(), b"exact bytes");
        discard_saved_attachment(&path);
        assert!(!Path::new(&path).exists() && !Path::new(&path).parent().unwrap().exists());
        discard_saved_attachment(Path::new(&root).join("inside.bin").to_str().unwrap());
        assert!(Path::new(&root).join("inside.bin").exists());
        let sink = WorkspaceAttachmentSink::new(&root, &identity, "unfinished.bin").unwrap();
        let folder = Path::new(&root).join(&sink.folder);
        drop(sink);
        assert!(!folder.exists());
        #[cfg(unix)]
        {
            std::fs::remove_dir_all(Path::new(&root).join(".agentsassemble-attachments")).unwrap();
            std::os::unix::fs::symlink(
                outside.path(),
                Path::new(&root).join(".agentsassemble-attachments"),
            )
            .unwrap();
            assert!(WorkspaceAttachmentSink::new(&root, &identity, "denied.bin").is_err());
            assert_eq!(std::fs::read(external).unwrap(), b"outside");
        }
    }
}
