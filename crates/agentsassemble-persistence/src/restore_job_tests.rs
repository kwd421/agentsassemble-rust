use super::*;
use crate::{
    ServerOwnerAuthority, SqliteStore,
    backup_codec::{BackupManifest, measure_payload},
    restore_activation::RestoreReceipt,
};
use sqlx::Connection;

#[tokio::test]
async fn reservation_serializes_with_runtime_and_never_replaces_initialized_or_empty_files()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    let target = directory.path().join("runtime.sqlite3");
    let job = RestoreJob::begin(&target)?;
    let operation = job.journal.operation_id;
    assert!(matches!(
        SqliteStore::open_path(&target).await,
        Err(PersistenceError::WriterAlreadyActive(_))
    ));
    assert!(!target.exists());
    drop(job);
    assert!(matches!(
        SqliteStore::open_path(&target).await,
        Err(PersistenceError::RestorePending)
    ));
    assert!(!target.exists());
    assert!(RestoreJob::resume(&target, uuid::Uuid::new_v4()).is_err());
    RestoreJob::resume(&target, operation)?.cancel()?;
    assert!(RestoreJob::inspect(&target)?.is_none());
    assert!(!target.exists());
    let occupied = tempfile::tempdir()?;
    let database = occupied.path().join("runtime.sqlite3");
    let store = SqliteStore::open_path(&database).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Existing owner")
        .await?;
    store.close().await?;
    let before = std::fs::read(&database)?;
    let key = std::fs::read(occupied.path().join("central-directory/host-ed25519.pk8"))?;
    assert!(matches!(
        RestoreJob::begin(&database),
        Err(PersistenceError::InitializationNotAllowed)
    ));
    assert_eq!(std::fs::read(&database)?, before);
    assert_eq!(
        std::fs::read(occupied.path().join("central-directory/host-ed25519.pk8"))?,
        key
    );
    let empty = tempfile::tempdir()?;
    let empty_file = empty.path().join("runtime.sqlite3");
    std::fs::File::create(&empty_file)?;
    assert!(matches!(
        RestoreJob::begin(&empty_file),
        Err(PersistenceError::InitializationNotAllowed)
    ));
    assert_eq!(std::fs::metadata(empty_file)?.len(), 0);
    Ok(())
}

#[tokio::test]
async fn prepared_move_resumes_exact_commit_and_opens_only_with_new_keys()
-> Result<(), Box<dyn std::error::Error>> {
    let source_dir = tempfile::tempdir()?;
    let source = SqliteStore::open_path(&source_dir.path().join("source.sqlite")).await?;
    source
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Move owner")
        .await?;
    source
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "Room")
        .await?;
    let old = source.host_identity().await?;
    let payload = source_dir.path().join("payload.sqlite");
    source
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &source_dir.path().join("snapshot.sqlite"),
            &payload,
        )
        .await?;
    let mut read = crate::backup_database::open_input(&payload).await?;
    let (rooms, messages, assets) = crate::restore_preparation::counts(&mut read).await?;
    read.close().await?;
    let (database_bytes, database_sha256) = measure_payload(&mut std::fs::File::open(&payload)?)?;
    let manifest = BackupManifest {
        schema_version: crate::schema_version::CURRENT_SCHEMA_VERSION,
        server_id: old.server_id().into(),
        server_name: "Move owner".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner-person".into(),
        source_epoch: "old-epoch".into(),
        source_key_fingerprint: "o".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let target_dir = tempfile::tempdir()?;
    let target = target_dir.path().join("runtime.sqlite3");
    let mut job = RestoreJob::begin(&target)?;
    let operation = job.journal.operation_id;
    job.prepare(&payload, manifest).await?;
    let new_identity = job.host_identity().await?;
    assert_eq!(new_identity.server_id(), old.server_id());
    assert_ne!(new_identity.private_key_pkcs8(), old.private_key_pkcs8());
    assert_ne!(new_identity.session_hmac_key(), old.session_hmac_key());
    job.bind_switch("confirmed-current-epoch", &"n".repeat(43))?;
    assert!(job.bind_switch("another-epoch", &"n".repeat(43)).is_err());
    job.mark_switch_unknown()?;
    drop(job);
    assert!(RestoreJob::resume(&target, operation)?.cancel().is_err());
    let mut resumed = RestoreJob::resume(&target, operation)?;
    assert!(resumed.activate().await.is_err());
    let mut receipt = RestoreReceipt {
        endpoint_generation: 4_000_000_000_000,
        operation_id: operation.to_string(),
        expected_epoch: "confirmed-current-epoch".into(),
        new_key_fingerprint: "n".repeat(43),
        registration_epoch: "committed-epoch".into(),
        status: "superseded".into(),
    };
    assert!(resumed.record_commit(&receipt).is_err());
    receipt.status = "committed".into();
    resumed.record_commit(&receipt)?;
    drop(resumed);
    let mut resumed = RestoreJob::resume(&target, operation)?;
    // A conflict after central commit must preserve unrelated bytes and resume
    // exactly the same key after the fixture removes its conflicting file.
    std::fs::write(&target, b"unrelated target")?;
    assert!(resumed.activate().await.is_err());
    assert_eq!(std::fs::read(&target)?, b"unrelated target");
    assert_eq!(resumed.journal.phase, RestorePhase::Activating);
    drop(resumed);
    std::fs::remove_file(&target)?;
    let mut resumed = RestoreJob::resume(&target, operation)?;
    resumed.activate().await?;
    drop(resumed);
    let restored = SqliteStore::open_path(&target).await?;
    assert!(restored.room_exists("general").await?);
    assert_eq!(
        restored.host_identity().await?.private_key_pkcs8(),
        new_identity.private_key_pkcs8()
    );
    assert_eq!(
        restored.registration_epoch().await?,
        Some("committed-epoch".into())
    );
    assert!(restored.next_central_endpoint_generation().await? > receipt.endpoint_generation);
    let endpoint = restored
        .reserve_central_endpoint_event("committed-epoch", "https://new.example", 1)
        .await?;
    assert!(
        restored
            .acknowledge_restored_endpoint(&endpoint, "wrong-key")
            .await
            .is_err()
    );
    assert_eq!(
        RestoreJob::inspect(&target)?.ok_or("missing job")?.phase,
        RestorePhase::Activated
    );
    restored
        .finish_central_endpoint_event(&endpoint, true)
        .await?;
    restored
        .acknowledge_restored_endpoint(&endpoint, &receipt.new_key_fingerprint)
        .await?;
    assert_eq!(
        RestoreJob::inspect(&target)?.ok_or("missing job")?.phase,
        RestorePhase::Ready
    );
    assert!(
        !target_dir
            .path()
            .join(format!(".server-move-{operation}"))
            .exists()
    );
    restored.close().await?;
    assert!(RestoreJob::begin(&target).is_err());
    assert_eq!(
        source.host_identity().await?.private_key_pkcs8(),
        old.private_key_pkcs8()
    );
    source.close().await?;
    Ok(())
}
