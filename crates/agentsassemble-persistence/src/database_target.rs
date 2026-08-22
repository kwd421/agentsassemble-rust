use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use fs2::FileExt;
use same_file::Handle;
use sqlx::sqlite::SqliteConnectOptions;

use crate::PersistenceError;

pub(crate) struct PreparedDatabase {
    pub(crate) options: SqliteConnectOptions,
    pub(crate) writer_lease: Option<Arc<File>>,
    pub(crate) created: bool,
    identity: Option<Handle>,
    canonical_path: Option<PathBuf>,
}

impl PreparedDatabase {
    pub(crate) fn from_url(database_url: &str) -> Result<Self, PersistenceError> {
        let options = SqliteConnectOptions::from_str(database_url)?.foreign_keys(true);
        if url_is_memory(database_url) {
            return Ok(Self {
                options,
                writer_lease: None,
                created: true,
                identity: None,
                canonical_path: None,
            });
        }
        prepare_file(options)
    }

    pub(crate) fn revalidate(&self) -> Result<(), PersistenceError> {
        let (Some(expected), Some(path)) = (&self.identity, &self.canonical_path) else {
            return Ok(());
        };
        let current = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .and_then(Handle::from_file)
            .map_err(PersistenceError::WriterLease)?;
        if &current != expected {
            return Err(PersistenceError::UnsafeDatabasePath(
                "database identity changed while ownership was being established",
            ));
        }
        Ok(())
    }
}

fn url_is_memory(database_url: &str) -> bool {
    let Some(specification) = database_url.strip_prefix("sqlite:") else {
        return false;
    };
    let (database, query) = specification
        .split_once('?')
        .map_or((specification, ""), |(database, query)| (database, query));
    database == ":memory:"
        || url::form_urlencoded::parse(query.as_bytes())
            .any(|(key, value)| key == "mode" && value == "memory")
}

fn prepare_file(options: SqliteConnectOptions) -> Result<PreparedDatabase, PersistenceError> {
    let requested = options.get_filename();
    let parent = requested
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .canonicalize()
        .map_err(PersistenceError::WriterLease)?;
    validate_private_directory(&parent)?;
    let filename = requested
        .file_name()
        .ok_or(PersistenceError::UnsafeDatabasePath(
            "database path has no filename",
        ))?;
    let resolved = parent.join(filename);
    reject_symlink(&resolved)?;
    let (file, created) = open_database_identity(&resolved)?;
    validate_database_file(&file, created)?;
    let canonical = resolved
        .canonicalize()
        .map_err(PersistenceError::WriterLease)?;
    let identity = Handle::from_file(file).map_err(PersistenceError::WriterLease)?;
    let lease = Arc::new(acquire_writer_lease(&canonical)?);
    let prepared = PreparedDatabase {
        options: options.filename(&canonical),
        writer_lease: Some(lease),
        created,
        identity: Some(identity),
        canonical_path: Some(canonical),
    };
    prepared.revalidate()?;
    Ok(prepared)
}

fn open_database_identity(path: &Path) -> Result<(File, bool), PersistenceError> {
    match OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(path)
    {
        Ok(file) => Ok((file, true)),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map(|file| (file, false))
            .map_err(PersistenceError::WriterLease),
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

fn reject_symlink(path: &Path) -> Result<(), PersistenceError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(
            PersistenceError::UnsafeDatabasePath("database path may not be a symbolic link"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

fn validate_database_file(file: &File, created: bool) -> Result<(), PersistenceError> {
    let metadata = file.metadata().map_err(PersistenceError::WriterLease)?;
    if !metadata.is_file() {
        return Err(PersistenceError::UnsafeDatabasePath(
            "database authority must be a regular file",
        ));
    }
    validate_link_count(&metadata)?;
    if created {
        set_private_file_permissions(file)?;
    } else {
        validate_private_file_permissions(&metadata)?;
    }
    Ok(())
}

fn acquire_writer_lease(database_path: &Path) -> Result<File, PersistenceError> {
    let mut lock_name = OsString::from(database_path.as_os_str());
    lock_name.push(".writer.lock");
    let lock_path = PathBuf::from(lock_name);
    reject_symlink(&lock_path)?;
    let (file, _) = open_database_identity(&lock_path)?;
    let metadata = file.metadata().map_err(PersistenceError::WriterLease)?;
    if !metadata.is_file() {
        return Err(PersistenceError::UnsafeDatabasePath(
            "writer lease authority must be a regular file",
        ));
    }
    validate_link_count(&metadata)?;
    set_private_file_permissions(&file)?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Err(PersistenceError::WriterAlreadyActive(lock_path))
        }
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

#[cfg(unix)]
fn validate_private_directory(path: &Path) -> Result<(), PersistenceError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = path
        .metadata()
        .map_err(PersistenceError::WriterLease)?
        .permissions()
        .mode();
    if mode & 0o022 == 0 {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeDatabasePath(
            "database directory must not grant group or other write access",
        ))
    }
}

#[cfg(not(unix))]
fn validate_private_directory(_path: &Path) -> Result<(), PersistenceError> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(file: &File) -> Result<(), PersistenceError> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(PersistenceError::WriterLease)
}

#[cfg(not(unix))]
fn set_private_file_permissions(_file: &File) -> Result<(), PersistenceError> {
    Ok(())
}

#[cfg(unix)]
fn validate_private_file_permissions(metadata: &std::fs::Metadata) -> Result<(), PersistenceError> {
    use std::os::unix::fs::PermissionsExt;

    if metadata.permissions().mode().trailing_zeros() >= 6 {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeDatabasePath(
            "database and lease files must not grant group or other access",
        ))
    }
}

#[cfg(not(unix))]
fn validate_private_file_permissions(
    _metadata: &std::fs::Metadata,
) -> Result<(), PersistenceError> {
    Ok(())
}

#[cfg(unix)]
fn validate_link_count(metadata: &std::fs::Metadata) -> Result<(), PersistenceError> {
    use std::os::unix::fs::MetadataExt;

    if metadata.nlink() == 1 {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeDatabasePath(
            "hard-linked database authorities are not supported",
        ))
    }
}

#[cfg(not(unix))]
fn validate_link_count(_metadata: &std::fs::Metadata) -> Result<(), PersistenceError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::url_is_memory;

    #[test]
    fn memory_mode_is_parsed_as_a_query_parameter_not_a_substring() {
        assert!(url_is_memory("sqlite::memory:"));
        assert!(url_is_memory(
            "sqlite:file:fixture?cache=shared&mode=memory"
        ));
        assert!(!url_is_memory("sqlite://folder/mode=memory.sqlite3"));
        assert!(!url_is_memory("sqlite:file:fixture?mode=rw"));
    }
}
