//! Offline installation reset keeps identity bindings, never a copy of old product data.
use crate::{PersistenceError, SqliteStore, database_target::PreparedDatabase};
use serde::Serialize;
use sqlx::{Row, sqlite::SqlitePoolOptions};
use std::{
    fs::{File, OpenOptions},
    path::Path,
};

#[derive(Serialize)]
pub struct AccountDataResetOutcome {
    pub reset: bool,
    pub cleanup_errors: Vec<String>,
}

impl SqliteStore {
    /// Explicit offline destructive reset. Caller must stop and reap its runtime first.
    /// # Errors
    /// Before atomic replacement, any failure retains the original product database.
    /// After replacement, cleanup/durability failures are returned in the exact outcome.
    pub async fn reset_deleted_account_data(
        path: &Path,
    ) -> Result<AccountDataResetOutcome, PersistenceError> {
        let parent = path.parent().ok_or_else(unsafe_path)?;
        if parent
            .symlink_metadata()
            .map_err(PersistenceError::WriterLease)?
            .file_type()
            .is_symlink()
            || parent
                .canonicalize()
                .map_err(PersistenceError::WriterLease)?
                != parent
            || path.file_name() != Some(std::ffi::OsStr::new("runtime.sqlite3"))
        {
            return Err(unsafe_path());
        }
        let old = Self::open_path(path).await?; // holds the target writer lease until all reset work ends
        let outcome = reset_with_lease(&old, path).await;
        old.close().await?;
        outcome
    }
}

async fn reset_with_lease(
    old: &SqliteStore,
    path: &Path,
) -> Result<AccountDataResetOutcome, PersistenceError> {
    let restriction = old.hosting_restriction().await?.ok_or_else(unsafe_path)?;
    if restriction != "account_deleted" && restriction != "retired" {
        return Err(unsafe_path());
    }
    let pending: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM central_member_removals WHERE phase!='complete')",
    )
    .fetch_one(&old.pool)
    .await?;
    if pending {
        return Err(unsafe_path());
    }
    let identity = old.host_identity().await?;
    let epoch = old.registration_epoch().await?.ok_or_else(unsafe_path)?;
    let nonce: String =
        sqlx::query_scalar("SELECT nonce FROM runtime_host_initialization WHERE singleton=1")
            .fetch_one(&old.pool)
            .await?;
    let parent = path.parent().ok_or_else(unsafe_path)?;
    let temporary = tempfile::Builder::new()
        .prefix(".account-deletion-")
        .suffix(".sqlite3")
        .tempfile_in(parent)
        .map_err(PersistenceError::WriterLease)?;
    let result = replace_database(
        old,
        path,
        &temporary,
        identity.server_id(),
        &nonce,
        &epoch,
        &restriction,
    )
    .await;
    let mut cleanup_errors = Vec::new();
    cleanup_temporary(temporary.path(), &mut cleanup_errors);
    match result {
        Ok(mut outcome) => {
            outcome.cleanup_errors.extend(cleanup_errors);
            Ok(outcome)
        }
        Err(error) if cleanup_errors.is_empty() => Err(error),
        Err(error) => Err(PersistenceError::WriterLease(std::io::Error::other(
            format!("{error}; temporary cleanup failed"),
        ))),
    }
}

async fn replace_database(
    old: &SqliteStore,
    path: &Path,
    temporary: &tempfile::NamedTempFile,
    server_id: &str,
    nonce: &str,
    epoch: &str,
    restriction: &str,
) -> Result<AccountDataResetOutcome, PersistenceError> {
    let parent = path.parent().ok_or_else(unsafe_path)?;
    crate::private_fs::secure_file(temporary.as_file(), temporary.path())
        .map_err(PersistenceError::WriterLease)?;
    let prepared = PreparedDatabase::from_path(temporary.path())?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(prepared.options.clone().create_if_missing(false))
        .await?;
    prepared.revalidate()?;
    let fresh = SqliteStore::from_owned_pool(pool, prepared, old.host_key.clone(), true);
    let built = build_fresh(&fresh, server_id, nonce, epoch, restriction).await;
    let closed = fresh.close().await;
    let mut cleanup_errors = Vec::new();
    built.and(closed)?;
    temporary
        .as_file()
        .sync_all()
        .map_err(PersistenceError::WriterLease)?;
    checkpoint(old).await?;
    old.pool.close().await;
    // Both names are installation-owned regular private single-link files. Keep
    // the target lease and recheck identity immediately before atomic replacement.
    validate_file(path)?;
    validate_file(temporary.path())?;
    let current = same_file::Handle::from_path(path).map_err(PersistenceError::WriterLease)?;
    if old.database_identity.as_deref() != Some(&current) {
        return Err(unsafe_path());
    }
    std::fs::rename(temporary.path(), path).map_err(PersistenceError::WriterLease)?;
    if File::open(parent).and_then(|file| file.sync_all()).is_err() {
        cleanup_errors.push("방 데이터는 지웠지만 디렉터리 저장 확인에 실패했어요.".into());
    }
    for name in [
        "runtime.sqlite3-wal",
        "runtime.sqlite3-shm",
        "room-directory-v1.json",
    ] {
        if remove_exact(&parent.join(name)).is_err() {
            cleanup_errors.push(format!("방 데이터는 지웠지만 {name} 정리에 실패했어요."));
        }
    }
    Ok(AccountDataResetOutcome {
        reset: true,
        cleanup_errors,
    })
}

async fn build_fresh(
    store: &SqliteStore,
    server_id: &str,
    nonce: &str,
    epoch: &str,
    restriction: &str,
) -> Result<(), PersistenceError> {
    crate::store_open::install_initialization_marker(&store.pool, nonce).await?;
    store.initialize().await?; // current schema/identity/bootstrap owners, no old row replay
    let mut tx = store.pool.begin_with("BEGIN IMMEDIATE").await?;
    sqlx::query("UPDATE runtime_host_identity SET server_id=? WHERE singleton=1")
        .bind(server_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runtime_metadata SET value=? WHERE key='server_id'")
        .bind(server_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES ('central_registration_epoch',?),('hosting_restriction',?)").bind(epoch).bind(restriction).execute(&mut *tx).await?;
    tx.commit().await?;
    if store.host_identity().await?.server_id() != server_id
        || store.registration_epoch().await?.as_deref() != Some(epoch)
        || store.hosting_restriction().await?.as_deref() != Some(restriction)
        || store.local_bootstrap_status().await?.phase != crate::LocalBootstrapPhase::Empty
    {
        return Err(unsafe_path());
    }
    for table in crate::schema::tables().filter(|table| {
        !matches!(
            table.name,
            "runtime_metadata"
                | "runtime_host_identity"
                | "runtime_host_initialization"
                | "local_bootstrap_authority"
        )
    }) {
        // Identifiers come only from the immutable schema owner, never SQLite/user rows.
        let query = format!("SELECT COUNT(*) FROM {}", table.name);
        let count: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(query))
            .fetch_one(&store.pool)
            .await?;
        if count != 0 {
            return Err(unsafe_path());
        }
    }
    checkpoint(store).await
}
async fn checkpoint(store: &SqliteStore) -> Result<(), PersistenceError> {
    let row = sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .fetch_one(&store.pool)
        .await?;
    if row.try_get::<i64, _>(0)? != 0 {
        return Err(unsafe_path());
    }
    Ok(())
}
fn validate_file(path: &Path) -> Result<(), PersistenceError> {
    let metadata = path
        .symlink_metadata()
        .map_err(PersistenceError::WriterLease)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(unsafe_path());
    }
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(PersistenceError::WriterLease)?;
    crate::database_target::validate_link_count(&file, &metadata)?;
    if !crate::private_fs::validate_file(&file, path).map_err(PersistenceError::WriterLease)? {
        return Err(unsafe_path());
    }
    Ok(())
}
fn remove_exact(path: &Path) -> Result<(), PersistenceError> {
    match path.symlink_metadata() {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(PersistenceError::WriterLease(error)),
        Ok(_) => {}
    }
    validate_file(path)?;
    std::fs::remove_file(path).map_err(PersistenceError::WriterLease)
}
fn cleanup_temporary(path: &Path, errors: &mut Vec<String>) {
    for suffix in ["-wal", "-shm", ".writer.lock"] {
        let mut name = path.as_os_str().to_os_string();
        name.push(suffix);
        if remove_exact(Path::new(&name)).is_err() {
            errors.push("이 작업의 임시 파일 정리에 실패했어요.".into());
        }
    }
}
fn unsafe_path() -> PersistenceError {
    PersistenceError::UnsafeDatabasePath(
        "offline reset requires exact stopped account-deleted installation custody",
    )
}

#[cfg(test)]
#[path = "account_data_reset_tests.rs"]
mod tests;
