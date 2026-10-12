use super::*;
use crate::{
    backup_database::{
        archive_tests::{archive, assert_product_rows},
        open_input,
    },
    restore_activation::RestoreReceipt,
    schema_version::CURRENT_SCHEMA_VERSION,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn historical_archives_upgrade_before_prepared_and_activate_with_all_product_bytes()
-> TestResult {
    for version in crate::archive_schema::FLOOR..=CURRENT_SCHEMA_VERSION {
        let directory = tempfile::tempdir()?;
        let (payload, manifest) = archive(directory.path(), version).await?;
        let destination = tempfile::tempdir()?;
        let target = destination.path().join("runtime.sqlite3");
        let mut job = RestoreJob::begin(&target)?;
        let operation = job.journal.operation_id;
        job.prepare(&payload, manifest.clone()).await?;
        assert_eq!(job.journal.phase, RestorePhase::Prepared);
        assert!(!target.exists());
        let staged = job.stage.join("runtime.sqlite3");
        assert_product_rows(&payload, &staged).await?;
        assert_current_and_searchable(&staged).await?;
        assert!(job.prepare(&payload, manifest).await.is_err());
        let key = job.host_identity().await?;
        assert_eq!(
            key.server_id(),
            job.journal.manifest.as_ref().ok_or("manifest")?.server_id
        );
        assert!(
            std::fs::read(directory.path().join("central-directory/host-ed25519.pk8"))?
                != std::fs::read(job.stage.join("central-directory/host-ed25519.pk8"))?,
            "restore reused the source key"
        );
        // Prepared is durable; resuming must keep its upgraded data and new key.
        drop(job);
        let mut resumed = RestoreJob::resume(&target, operation)?;
        assert_eq!(resumed.journal.phase, RestorePhase::Prepared);
        assert!(
            resumed.host_identity().await?.private_key_pkcs8() == key.private_key_pkcs8(),
            "Prepared resume changed its key"
        );
        resumed.bind_switch("epoch", &"b".repeat(43))?;
        resumed.mark_switch_unknown()?;
        resumed.record_commit(&RestoreReceipt {
            endpoint_generation: 100,
            operation_id: operation.to_string(),
            expected_epoch: "epoch".into(),
            new_key_fingerprint: "b".repeat(43),
            registration_epoch: "new-epoch".into(),
            status: "committed".into(),
        })?;
        resumed.activate().await?;
        drop(resumed);
        assert_product_rows(&payload, &target).await?;
        assert_current_and_searchable(&target).await?;
    }
    Ok(())
}

#[tokio::test]
async fn failed_real_upgrade_stays_validating_and_resume_reconstructs_original_payload()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let (payload, manifest) = archive(directory.path(), 87).await?;
    let live = directory.path().join("live.sqlite");
    let live_before = std::fs::read(&live)?;
    let live_key = directory.path().join("central-directory/host-ed25519.pk8");
    let key_before = std::fs::read(&live_key)?;
    let payload_before = std::fs::read(&payload)?;
    let destination = tempfile::tempdir()?;
    let target = destination.path().join("runtime.sqlite3");
    let mut job = RestoreJob::begin(&target)?;
    let operation = job.journal.operation_id;
    job.fail_upgrade = true;
    let error = job
        .prepare(&payload, manifest.clone())
        .await
        .err()
        .ok_or("upgrade unexpectedly succeeded")?;
    assert!(
        matches!(error, PersistenceError::Database(ref error) if error.to_string().contains("injected migration failure"))
    );
    assert_eq!(job.journal.phase, RestorePhase::Validating);
    assert!(job.journal.manifest.is_none());
    assert!(job.bind_switch("epoch", &"b".repeat(43)).is_err());
    assert!(!target.exists());
    let staged = job.stage.join("runtime.sqlite3");
    let mut partial = open_input(&staged).await?;
    assert_eq!(crate::archive_schema::version(&mut partial).await?, 88);
    partial.close().await?;
    let failed_key = std::fs::read(job.stage.join("central-directory/host-ed25519.pk8"))?;
    drop(job);
    let mut resumed = RestoreJob::resume(&target, operation)?;
    assert_eq!(resumed.journal.phase, RestorePhase::Validating);
    resumed.prepare(&payload, manifest).await?;
    assert_eq!(resumed.journal.phase, RestorePhase::Prepared);
    assert_current_and_searchable(&staged).await?;
    assert_product_rows(&payload, &staged).await?;
    assert!(
        std::fs::read(resumed.stage.join("central-directory/host-ed25519.pk8"))? != failed_key,
        "retry reused the incomplete identity"
    );
    assert!(std::fs::read(&live)? == live_before, "source store changed");
    assert!(
        std::fs::read(&live_key)? == key_before,
        "source key changed"
    );
    assert!(std::fs::read(&payload)? == payload_before, "input changed");
    resumed.cancel()?;
    Ok(())
}

async fn assert_current_and_searchable(path: &Path) -> TestResult {
    let mut connection = open_input(path).await?;
    assert_eq!(
        crate::archive_schema::version(&mut connection).await?,
        CURRENT_SCHEMA_VERSION
    );
    assert_eq!(sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM room_message_search_phrase WHERE room_message_search_phrase MATCH 'Historical'"
    ).fetch_one(&mut connection).await?, 1);
    crate::backup_database::require_integrity(&mut connection).await?;
    connection.close().await?;
    Ok(())
}
