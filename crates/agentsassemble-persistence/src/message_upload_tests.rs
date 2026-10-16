use super::*;
use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
use sqlx::Connection;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn fixture(
    path: Option<&std::path::Path>,
) -> Result<(SqliteStore, MessageUploadAuthority), PersistenceError> {
    let store = if let Some(path) = path {
        SqliteStore::open_path(path).await?
    } else {
        SqliteStore::open("sqlite::memory:").await?
    };
    store
        .bootstrap_local_authority("1468c3c6-a7f5-4311-b262-a4e492db1032", "Host")
        .await?;
    store
        .create_room_for_local_operator(
            "6f218ed3-8fe3-4487-86e4-3120c41fdd9e",
            "general",
            "General",
        )
        .await?;
    let authority = MessageUploadAuthority::Local {
        room_id: "general".into(),
        user_id: LOCAL_OPERATOR_USER_ID.into(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.into(),
    };
    Ok((store, authority))
}
fn code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("Expected {expected}, got {error}"),
        Ok(_) => panic!("Expected {expected}, got success"),
    }
}
async fn policy(store: &SqliteStore, limit: usize, quota: u64) -> TestResult {
    let mut tx = store.pool.begin().await?;
    let principal = attachments::current_local_message_principal(
        &mut tx,
        "general",
        LOCAL_OPERATOR_USER_ID,
        LOCAL_OPERATOR_PARTICIPANT_ID,
    )
    .await?;
    tx.commit().await?;
    let settings = store.snapshot("general", 0, 100).await?.settings;
    store.execute_room_settings_update(crate::RoomMutationAuthority::TrustedPrincipal(&principal),&Uuid::new_v4().to_string(),&serde_json::json!({
        "expected_revision":agentsassemble_domain::public_settings(&settings)?.settings_revision,
        "attachment_file_limit_bytes":limit,"attachment_storage_limit_bytes":quota,
    })).await?;
    Ok(())
}

#[tokio::test]
async fn exact_resume_size_offsets_idempotent_finish_and_bind() -> TestResult {
    let (store, auth) = fixture(None).await?;
    let size = 50 * 1024 * 1024;
    policy(&store, size, 500 * 1024 * 1024).await?;
    let begun = store
        .begin_message_upload(
            &auth,
            "large",
            "../large.bin",
            "application/octet-stream",
            size,
        )
        .await?;
    assert_eq!(
        store
            .begin_message_upload(
                &auth,
                "large",
                "../large.bin",
                "application/octet-stream",
                size
            )
            .await?
            .handle,
        begun.handle
    );
    code(
        store
            .begin_message_upload(&auth, "large", "different.bin", "text/plain", size)
            .await,
        "upload_operation_conflict",
    );
    let handle = &begun.handle;
    for offset in [1, 65536] {
        code(
            store
                .append_message_upload(&auth, handle, offset, b"x")
                .await,
            "upload_offset_conflict",
        );
    }
    store
        .append_message_upload(&auth, handle, 0, &[7; 19])
        .await?;
    assert_eq!(
        store
            .message_upload_status(&auth, handle)
            .await?
            .accepted_offset,
        19
    );
    code(
        store
            .append_message_upload(&auth, handle, 0, &[7; 19])
            .await,
        "upload_offset_conflict",
    );
    code(
        store.append_message_upload(&auth, handle, 18, b"x").await,
        "upload_offset_conflict",
    );
    code(
        store.finish_message_upload(&auth, handle).await,
        "upload_incomplete",
    );
    // Simulate a disconnected data request: resume from its exact partial-piece status.
    let mut offset = 19;
    while offset < size {
        let bytes = vec![7; (size - offset).min(MESSAGE_UPLOAD_PIECE_BYTES)];
        offset = store
            .append_message_upload(&auth, handle, offset, &bytes)
            .await?;
    }
    code(
        store.append_message_upload(&auth, handle, size, b"x").await,
        "upload_size_exceeded",
    );
    let finished = store
        .finish_message_upload(&auth, handle)
        .await?
        .attachment
        .ok_or("attachment")?;
    assert_eq!(
        store.finish_message_upload(&auth, handle).await?.attachment,
        Some(finished.clone())
    );
    assert_eq!(
        store.message_upload_status(&auth, handle).await?.attachment,
        Some(finished.clone())
    );
    assert_eq!(finished.filename, "large.bin");
    assert!(!finished.is_image);
    let mut tx = store.pool.begin().await?;
    let principal = auth.resolve(&mut tx).await?;
    tx.commit().await?;
    let sent = store
        .execute_message(
            &principal,
            "bind-transfer",
            "message.send",
            &serde_json::json!({"content":"", "attachment_ids":[finished.id]}),
        )
        .await?;
    assert!(sent.event.extra.contains_key("attachments"));
    let mut connection = store.pool.acquire().await?;
    for sequence in 0..size.div_ceil(MESSAGE_UPLOAD_PIECE_BYTES) {
        assert_eq!(
            crate::room_file_content::RoomFileContent::Message
                .read(&mut connection, &finished.id, sequence)
                .await?,
            vec![7; MESSAGE_UPLOAD_PIECE_BYTES]
        );
    }
    Ok(())
}

#[tokio::test]
async fn reservations_exclusion_cleanup_policy_change_and_restart() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (store, auth) = fixture(Some(&directory.path().join("runtime.sqlite"))).await?;
    policy(&store, 50 * 1024 * 1024, 500 * 1024 * 1024).await?;
    for index in 0..9 {
        store
            .begin_message_upload(
                &auth,
                &format!("reserved-{index}"),
                "reserved.bin",
                "application/octet-stream",
                50 * 1024 * 1024,
            )
            .await?;
    }
    let first = store
        .begin_message_upload(
            &auth,
            "first",
            "file.bin",
            "application/octet-stream",
            25 * 1024 * 1024,
        )
        .await?;
    code(
        store
            .begin_message_upload(
                &auth,
                "quota",
                "file.bin",
                "application/octet-stream",
                26 * 1024 * 1024,
            )
            .await,
        "room_attachment_quota_reached",
    );
    let (a, b) = tokio::join!(
        store.begin_message_upload(
            &auth,
            "race-a",
            "file.bin",
            "application/octet-stream",
            25 * 1024 * 1024
        ),
        store.begin_message_upload(
            &auth,
            "race-b",
            "file.bin",
            "application/octet-stream",
            25 * 1024 * 1024
        )
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let winner = a.ok().or(b.ok()).ok_or("quota race winner")?;
    store.discard_message_upload(&winner.handle).await?;
    let mut tx = store.pool.begin().await?;
    let principal = auth.resolve(&mut tx).await?;
    tx.commit().await?;
    code(
        store
            .store_message_attachment(
                &principal,
                "legacy.bin",
                "application/octet-stream",
                vec![1; 26 * 1024 * 1024],
            )
            .await,
        "room_attachment_quota_reached",
    );
    code(
        store
            .execute_message(
                &principal,
                "incomplete-bind",
                "message.send",
                &serde_json::json!({"content":"", "attachment_ids":[first.attachment_id]}),
            )
            .await,
        "attachment_unavailable",
    );
    assert!(
        store
            .bound_message_attachment(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &first.attachment_id,
                |_, _| Ok(()),
            )
            .await
            .is_err()
    );
    assert_eq!(store.attachment_storage_usage().await?.total.files, 0);
    let snapshot = directory.path().join("snapshot.sqlite");
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(
            &crate::ServerOwnerAuthority::LocalOperator,
            &snapshot,
            &payload,
        )
        .await
        .map_err(|e| format!("export staging exclusion: {e}"))?;
    let mut archive = SqliteConnection::connect(&format!("sqlite://{}", payload.display())).await?;
    let tables: i64 =
        sqlx::query_scalar("SELECT count(*) FROM sqlite_schema WHERE name LIKE 'message_upload%'")
            .fetch_one(&mut archive)
            .await?;
    assert_eq!(tables, 0);
    assert!(
        sqlx::query("SELECT * FROM message_uploads")
            .fetch_all(&mut archive)
            .await
            .is_err()
    );
    sqlx::query("CREATE TABLE message_uploads(handle TEXT,content BLOB)")
        .execute(&mut archive)
        .await?;
    let mut target =
        crate::backup_database::create_payload(&directory.path().join("restore.sqlite")).await?;
    assert!(
        crate::backup_database::reconstruct(
            &mut archive,
            &mut target,
            true,
            crate::schema_version::CURRENT_SCHEMA_VERSION
        )
        .await
        .is_err()
    );
    for sequence in 0..400 {
        store
            .append_message_upload(
                &auth,
                &first.handle,
                sequence * MESSAGE_UPLOAD_PIECE_BYTES,
                &[1; MESSAGE_UPLOAD_PIECE_BYTES],
            )
            .await?;
    }
    policy(&store, 10 * 1024 * 1024, 500 * 1024 * 1024).await?;
    code(
        store.finish_message_upload(&auth, &first.handle).await,
        "attachment_too_large",
    );
    store.discard_message_upload(&first.handle).await?;
    policy(&store, 50 * 1024 * 1024, 500 * 1024 * 1024).await?;
    let second = store
        .begin_message_upload(
            &auth,
            "second",
            "file.bin",
            "application/octet-stream",
            50 * 1024 * 1024,
        )
        .await?;
    store
        .append_message_upload(&auth, &second.handle, 0, b"expiry tail")
        .await?;
    sqlx::query("UPDATE temp.message_uploads SET expires_at=0 WHERE handle=?")
        .bind(&second.handle)
        .execute(&store.pool)
        .await?;
    let prior = store
        .begin_message_upload(
            &auth,
            "third",
            "file.bin",
            "application/octet-stream",
            50 * 1024 * 1024,
        )
        .await?;
    let pieces: i64 = sqlx::query_scalar("SELECT count(*) FROM temp.message_upload_pieces")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(pieces, 0);
    sqlx::query(
        "UPDATE rooms SET room_json=json_set(room_json,'$.room_uid',?) WHERE room_id='general'",
    )
    .bind(Uuid::new_v4().to_string())
    .execute(&store.pool)
    .await?;
    code(
        store.message_upload_status(&auth, &prior.handle).await,
        "upload_missing",
    );
    let replacement = store
        .begin_message_upload(
            &auth,
            "third",
            "file.bin",
            "application/octet-stream",
            50 * 1024 * 1024,
        )
        .await?;
    assert_ne!(
        replacement.handle, prior.handle,
        "a replacement room cannot inherit a transfer"
    );
    store.close().await?;
    let reopened = SqliteStore::open_path(&directory.path().join("runtime.sqlite")).await?;
    let transfers: i64 = sqlx::query_scalar("SELECT count(*) FROM temp.message_uploads")
        .fetch_one(&reopened.pool)
        .await?;
    assert_eq!(transfers, 0);
    Ok(())
}

#[tokio::test]
async fn raster_preview_matches_legacy_and_invalid_raster_never_commits() -> TestResult {
    let (store, auth) = fixture(None).await?;
    let mut noise = 0x12345678_u32;
    let image = image::DynamicImage::ImageRgba8(image::ImageBuffer::from_fn(500, 500, |_, _| {
        noise ^= noise << 13;
        noise ^= noise >> 17;
        noise ^= noise << 5;
        image::Rgba([(noise >> 16) as u8, (noise >> 8) as u8, noise as u8, 255])
    }));
    let mut output = std::io::Cursor::new(Vec::new());
    image.write_to(&mut output, image::ImageFormat::Png)?;
    let bytes = output.into_inner();
    let transfer = store
        .begin_message_upload(
            &auth,
            "raster",
            "../raster.png",
            " IMAGE/PNG; charset=binary ",
            bytes.len(),
        )
        .await?;
    let mut offset = 0;
    for piece in bytes.chunks(MESSAGE_UPLOAD_PIECE_BYTES) {
        offset = store
            .append_message_upload(&auth, &transfer.handle, offset, piece)
            .await?;
    }
    let finished = store
        .finish_message_upload(&auth, &transfer.handle)
        .await?
        .attachment
        .ok_or("image")?;
    let legacy = store
        .store_local_message_attachment(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            "../raster.png",
            " IMAGE/PNG; charset=binary ",
            bytes.clone(),
        )
        .await?;
    assert_eq!(
        (
            &finished.filename,
            &finished.content_type,
            finished.size,
            finished.is_image
        ),
        (
            &legacy.filename,
            &legacy.content_type,
            legacy.size,
            legacy.is_image
        )
    );
    let preview = "SELECT content FROM room_message_attachment_previews WHERE attachment_id=?";
    let a: Option<Option<Vec<u8>>> = sqlx::query_scalar(preview)
        .bind(&finished.id)
        .fetch_optional(&store.pool)
        .await?;
    let b: Option<Option<Vec<u8>>> = sqlx::query_scalar(preview)
        .bind(&legacy.id)
        .fetch_optional(&store.pool)
        .await?;
    assert_eq!(a, b);
    assert!(
        a.flatten().is_some(),
        "upload validation must generate the large-image preview"
    );
    assert_eq!(
        crate::room_file_content::RoomFileContent::Message
            .collect(&mut *store.pool.acquire().await?, &finished.id, bytes.len())
            .await?,
        bytes
    );
    let bad = store
        .begin_message_upload(&auth, "bad", "bad.png", "image/png", 3)
        .await?;
    store
        .append_message_upload(&auth, &bad.handle, 0, b"bad")
        .await?;
    code(
        store.finish_message_upload(&auth, &bad.handle).await,
        "attachment_invalid_image",
    );
    assert!(
        store
            .message_upload_status(&auth, &bad.handle)
            .await?
            .attachment
            .is_none()
    );
    Ok(())
}

#[tokio::test]
#[ignore = "reproduces: cancelling a pool acquire during its idle ping closes the only connection and loses TEMP upload staging"]
async fn staged_upload_survives_idle_ping_cancellation() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (store, auth) = fixture(Some(&directory.path().join("runtime.sqlite"))).await?;
    policy(&store, 50 * 1024 * 1024, 500 * 1024 * 1024).await?;
    let begun = store
        .begin_message_upload(
            &auth,
            "interrupted",
            "file.bin",
            "application/octet-stream",
            1024,
        )
        .await?;
    store
        .append_message_upload(&auth, &begun.handle, 0, &[7; 512])
        .await?;
    // A request future dropped while suspended in the pool's idle ping is what a client
    // disconnect or select! cancellation does. Status is read-only, so a probe that
    // completes without suspending is simply retried.
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    let mut closed = false;
    for _ in 0..1_000 {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while store.pool.num_idle() == 0 {
                tokio::task::yield_now().await;
            }
        })
        .await?;
        let mut probe = Box::pin(store.message_upload_status(&auth, &begun.handle));
        if std::future::Future::poll(probe.as_mut(), &mut context).is_pending() {
            drop(probe);
            if store.pool.size() == 0 {
                closed = true;
                break;
            }
        }
    }
    assert!(closed, "no status probe was cancelled during the idle ping");
    let status = store.message_upload_status(&auth, &begun.handle).await?;
    assert_eq!(status.accepted_offset, 512);
    store
        .append_message_upload(&auth, &begun.handle, 512, &[7; 512])
        .await?;
    Ok(())
}
