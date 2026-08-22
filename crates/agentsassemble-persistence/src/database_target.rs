use std::{
    ffi::OsString,
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::Duration,
};

use fs2::FileExt;
use same_file::Handle;
use sqlx::sqlite::{SqliteConnectOptions, SqliteLockingMode};

use crate::{
    PersistenceError,
    private_fs::{secure_file, validate_directory, validate_file},
};

pub(crate) struct PreparedDatabase {
    pub(crate) options: SqliteConnectOptions,
    pub(crate) writer_lease: Option<Arc<File>>,
    pub(crate) created: bool,
    pub(crate) identity: Option<Arc<Handle>>,
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

    pub(crate) fn from_path(path: &Path) -> Result<Self, PersistenceError> {
        prepare_file(
            SqliteConnectOptions::new()
                .filename(path)
                .foreign_keys(true),
        )
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
        if current != **expected {
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
    let identity = Arc::new(Handle::from_file(file).map_err(PersistenceError::WriterLease)?);
    let lease = Arc::new(acquire_writer_lease(&canonical)?);
    let prepared = PreparedDatabase {
        options: options
            .filename(&canonical)
            .locking_mode(SqliteLockingMode::Exclusive)
            .busy_timeout(Duration::from_millis(250)),
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
    validate_link_count(file, &metadata)?;
    if created {
        secure_file(file).map_err(PersistenceError::WriterLease)?;
    } else if !validate_file(file).map_err(PersistenceError::WriterLease)? {
        return Err(PersistenceError::UnsafeDatabasePath(
            "database and lease files must be private to the current user",
        ));
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
    validate_link_count(&file, &metadata)?;
    secure_file(&file).map_err(PersistenceError::WriterLease)?;
    match FileExt::try_lock_exclusive(&file) {
        Ok(()) => Ok(file),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
            Err(PersistenceError::WriterAlreadyActive(lock_path))
        }
        Err(error) => Err(PersistenceError::WriterLease(error)),
    }
}

fn validate_private_directory(path: &Path) -> Result<(), PersistenceError> {
    if validate_directory(path).map_err(PersistenceError::WriterLease)? {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeDatabasePath(
            "database directory must be private to the current user",
        ))
    }
}

#[cfg(unix)]
fn validate_link_count(_file: &File, metadata: &std::fs::Metadata) -> Result<(), PersistenceError> {
    use std::os::unix::fs::MetadataExt;

    if metadata.nlink() == 1 {
        Ok(())
    } else {
        Err(PersistenceError::UnsafeDatabasePath(
            "hard-linked database authorities are not supported",
        ))
    }
}

#[cfg(windows)]
fn validate_link_count(file: &File, _metadata: &std::fs::Metadata) -> Result<(), PersistenceError> {
    match winapi_util::file::information(file)
        .map_err(PersistenceError::WriterLease)?
        .number_of_links()
    {
        1 => Ok(()),
        _ => Err(PersistenceError::UnsafeDatabasePath(
            "hard-linked database authorities are not supported",
        )),
    }
}

#[cfg(all(not(unix), not(windows)))]
fn validate_link_count(
    _file: &File,
    _metadata: &std::fs::Metadata,
) -> Result<(), PersistenceError> {
    Err(PersistenceError::UnsafeDatabasePath(
        "database hard-link validation is unsupported on this platform",
    ))
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
