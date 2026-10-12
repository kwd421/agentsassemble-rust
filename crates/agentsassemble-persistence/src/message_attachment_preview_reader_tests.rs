use super::*;
use agentsassemble_domain::AuthenticatedPrincipal;
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

fn authority(principal: &AuthenticatedPrincipal) -> MessageAttachmentReadAuthority {
    MessageAttachmentReadAuthority::Local {
        room_id: principal.room_id.clone(),
        user_id: principal.principal_id.clone(),
        participant_id: principal.participant_id.clone(),
    }
}

#[tokio::test]
async fn cache_upload_lazy_concurrency_restore_and_cascade_share_original_authority() -> TestResult
{
    let (_, principal) = crate::message_attachment_tests::fixture().await;
    let source_directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&source_directory.path().join("source.sqlite")).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let original = crate::raster_preview_tests::large_png(true, true)?;
    let attachment = store
        .store_message_attachment(&principal, "large.png", "image/png", original.clone())
        .await?;
    let size: i64 = sqlx::query_scalar(
        "SELECT length(content) FROM room_message_attachment_previews WHERE attachment_id=?",
    )
    .bind(&attachment.id)
    .fetch_one(&store.pool)
    .await?;
    assert!((1..=crate::raster_preview::MAX_BYTES as i64).contains(&size));
    assert!(
        store
            .open_message_attachment_preview_reader(
                authority(&principal),
                &attachment.id,
                |_| Ok(()),
                |_, _| Ok(())
            )
            .await
            .is_err()
    );
    assert!(
        store
            .open_message_attachment_reader(authority(&principal), &attachment.id, |_| Ok(()))
            .await
            .is_err()
    );
    store
        .execute_message(
            &principal,
            &uuid::Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"", "attachment_ids":[attachment.id]}),
        )
        .await?;
    // A pre-preview attachment has original metadata/chunks but no derived row.
    sqlx::query("DELETE FROM room_message_attachment_previews")
        .execute(&store.pool)
        .await?;
    let generated = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(tokio::sync::Barrier::new(3));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let (store, principal, id, barrier, generated) = (
            store.clone(),
            principal.clone(),
            attachment.id.clone(),
            barrier.clone(),
            generated.clone(),
        );
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store
                .open_message_attachment_preview_reader(
                    authority(&principal),
                    &id,
                    |_| Ok(()),
                    |_, _| {
                        generated.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    },
                )
                .await
        }));
    }
    barrier.wait().await;
    for task in tasks {
        let (reader, ()) = task.await??;
        assert_eq!(reader.metadata().size, size as usize);
        assert!(!reader.read_piece(0).await?.is_empty());
    }
    assert_eq!(generated.load(Ordering::SeqCst), 1);
    let directory = tempfile::tempdir()?;
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(
            &crate::ServerOwnerAuthority::LocalOperator,
            &directory.path().join("snapshot.sqlite"),
            &payload,
        )
        .await
        .map_err(|e| format!("export: {e}"))?;
    let (rooms, messages, assets) = crate::backup_database::payload_counts(&payload).await?;
    let (database_bytes, database_sha256) =
        crate::backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
    let manifest = crate::backup_codec::BackupManifest {
        schema_version: crate::schema_version::CURRENT_SCHEMA_VERSION,
        server_id: store.host_identity().await?.server_id().into(),
        server_name: "Preview restore".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner".into(),
        source_epoch: "epoch".into(),
        source_key_fingerprint: "x".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let mut job = crate::restore_job::RestoreJob::begin(&directory.path().join("runtime.sqlite3"))?;
    job.prepare(&payload, manifest).await?;
    let restored = SqliteStore::open_path(&job.stage().join("runtime.sqlite3")).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM room_message_attachment_previews")
        .fetch_one(&restored.pool)
        .await?;
    assert_eq!(count, 0);
    let (reader, ()) = restored
        .open_message_attachment_preview_reader(
            authority(&principal),
            &attachment.id,
            |_| Ok(()),
            |_, _| Ok(()),
        )
        .await?;
    assert_eq!(reader.metadata().size, size as usize);
    let (full, ()) = restored
        .open_message_attachment_reader(authority(&principal), &attachment.id, |_| Ok(()))
        .await?;
    assert_eq!(full.read_piece(0).await?, original[..65536]);
    sqlx::query("DELETE FROM room_message_attachments")
        .execute(&restored.pool)
        .await?;
    assert!(reader.read_piece(0).await.is_err());
    assert!(full.read_piece(0).await.is_err());
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM room_message_attachment_previews")
        .fetch_one(&restored.pool)
        .await?;
    assert_eq!(count, 0);
    drop(reader);
    drop(full);
    restored.close().await?;
    job.cancel()?;
    Ok(())
}
