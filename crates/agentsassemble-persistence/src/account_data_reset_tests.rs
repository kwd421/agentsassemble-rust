use super::*;

async fn fixture(root: &Path) -> Result<(SqliteStore, String), PersistenceError> {
    let store = SqliteStore::open_path(&root.join("runtime.sqlite3")).await?;
    store
        .bootstrap_local_authority("884252b0-19a6-48dd-bc89-50cfef3f59c5", "Private operator")
        .await?;
    store.set_registration_epoch(Some("old-epoch")).await?;
    // Terminal fence is a fixture of a completed, physically settled removal.
    store.retire_hosting_incarnation(Some("old-epoch")).await?;
    let id = store.host_identity().await?.server_id().to_string();
    Ok((store, id))
}

#[tokio::test]
async fn offline_reset_refuses_live_writer_then_keeps_exact_keys_and_fence_without_product_data()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let canonical = root.path().canonicalize()?;
    let path = canonical.join("runtime.sqlite3");
    let (store, id) = fixture(&canonical).await?;
    assert!(matches!(
        SqliteStore::reset_deleted_account_data(&path).await,
        Err(PersistenceError::WriterAlreadyActive(_))
    ));
    let key_path = root.path().join("central-directory/host-ed25519.pk8");
    let key = std::fs::read(&key_path)?;
    store.close().await?;
    let outcome = SqliteStore::reset_deleted_account_data(&path).await?;
    assert!(outcome.reset);
    assert!(outcome.cleanup_errors.is_empty());
    let keys_preserved = std::fs::read(&key_path)? == key;
    assert!(keys_preserved);
    let reopened = SqliteStore::open_path(&path).await?;
    assert_eq!(reopened.host_identity().await?.server_id(), id);
    assert_eq!(
        reopened.registration_epoch().await?.as_deref(),
        Some("old-epoch")
    );
    assert_eq!(
        reopened.hosting_restriction().await?.as_deref(),
        Some("retired")
    );
    assert_eq!(
        reopened.local_bootstrap_status().await?.phase,
        crate::LocalBootstrapPhase::Empty
    );
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_profiles")
        .fetch_one(&reopened.pool)
        .await?;
    assert_eq!(count, 0);
    reopened.close().await?;
    assert_eq!(
        crate::inspect_host_identity(&path).await?,
        Some((id, None, Some("retired".to_owned())))
    );
    assert!(
        !std::fs::read_dir(root.path())?
            .filter_map(Result::ok)
            .any(|entry| entry
                .file_name()
                .to_string_lossy()
                .starts_with(".account-deletion-"))
    );
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn unsafe_root_and_database_fail_before_replacement_and_cache_failure_reports_applied_reset()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let canonical = root.path().canonicalize()?;
    let path = canonical.join("runtime.sqlite3");
    let (store, _) = fixture(&canonical).await?;
    store.close().await?;
    let outside = tempfile::tempdir()?;
    let alias = outside.path().join("linked-root");
    std::os::unix::fs::symlink(root.path(), &alias)?;
    assert!(
        SqliteStore::reset_deleted_account_data(&alias.join("runtime.sqlite3"))
            .await
            .is_err()
    );
    let linked = outside.path().join("linked-db");
    std::fs::hard_link(&path, &linked)?;
    assert!(
        SqliteStore::reset_deleted_account_data(&path)
            .await
            .is_err()
    );
    std::fs::remove_file(linked)?;
    let untouched = outside.path().join("unrelated");
    std::fs::write(&untouched, b"keep")?;
    std::os::unix::fs::symlink(&untouched, root.path().join("room-directory-v1.json"))?;
    let outcome = SqliteStore::reset_deleted_account_data(&path).await?;
    assert!(outcome.reset);
    assert_eq!(outcome.cleanup_errors.len(), 1);
    assert!(
        root.path()
            .join("room-directory-v1.json")
            .symlink_metadata()?
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(untouched)?, b"keep");
    let reopened = SqliteStore::open_path(&path).await?;
    assert_eq!(
        reopened.local_bootstrap_status().await?.phase,
        crate::LocalBootstrapPhase::Empty
    );
    reopened.close().await?;
    Ok(())
}
