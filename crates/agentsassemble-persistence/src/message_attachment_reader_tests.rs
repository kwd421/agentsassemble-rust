use super::*;
use agentsassemble_domain::InviteScope;
use serde_json::json;
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn piece_reads_reauthorize_after_deletion_and_access_loss() -> TestResult {
    for mutation in [
        "DELETE FROM room_message_attachments",
        "DELETE FROM room_events",
        "DELETE FROM rooms",
        "DELETE FROM participants WHERE participant_id='session-guest'",
        "UPDATE human_room_sessions SET state='ended'",
        "DELETE FROM user_profiles WHERE participant_id='session-guest'",
    ] {
        let (store, _) =
            crate::human_session_authority_tests::admitted_fixture(InviteScope::ReadOnly).await;
        let (_, principal) = crate::message_attachment_tests::fixture().await;
        let bytes = crate::raster_preview_tests::large_png(true, false)?;
        let attachment = store
            .store_message_attachment(&principal, "pixels.png", "image/png", bytes.clone())
            .await?;
        store
            .execute_message(
                &principal,
                "bind",
                "message.send",
                &json!({"content":"", "attachment_ids":[attachment.id]}),
            )
            .await?;
        let fingerprint = crate::human_session_authority_tests::session_fingerprint(&store).await;
        let session = store.authorize_human_session(&fingerprint).await?;
        let (reader, ()) = store
            .open_message_attachment_reader(
                MessageAttachmentReadAuthority::Session(Box::new(RoomSessionAuthorization::Human(
                    session.clone(),
                ))),
                &attachment.id,
                |_| Ok(()),
            )
            .await?;
        let (preview, ()) = store
            .open_message_attachment_preview_reader(
                MessageAttachmentReadAuthority::Session(Box::new(RoomSessionAuthorization::Human(
                    session,
                ))),
                &attachment.id,
                |_| Ok(()),
                |_, _| Ok(()),
            )
            .await?;
        assert!(!preview.read_piece(0).await?.is_empty());
        assert_eq!(reader.read_piece(0).await?, bytes[..65536]);
        // Releasing the first read permits an unrelated writer before any next read.
        sqlx::query(mutation).execute(&store.pool).await?;
        assert!(reader.read_piece(1).await.is_err(), "{mutation}");
        assert!(preview.read_piece(1).await.is_err(), "preview: {mutation}");
    }
    Ok(())
}

#[tokio::test]
async fn local_piece_reader_is_exact_and_does_not_retain_the_only_connection() -> TestResult {
    let (store, principal) = crate::message_attachment_tests::fixture().await;
    let bytes: Vec<u8> = (0..65536 + 19).map(|n| u8::try_from(n % 251).unwrap()).collect();
    let attachment = store
        .store_message_attachment(
            &principal,
            "pieces.bin",
            "application/octet-stream",
            bytes.clone(),
        )
        .await?;
    store
        .execute_message(
            &principal,
            "bind",
            "message.send",
            &json!({"content":"", "attachment_ids":[attachment.id]}),
        )
        .await?;
    let (reader, ()) = store
        .open_message_attachment_reader(
            MessageAttachmentReadAuthority::Local {
                room_id: principal.room_id,
                user_id: principal.principal_id,
                participant_id: principal.participant_id,
            },
            &attachment.id,
            |_| Ok(()),
        )
        .await?;
    assert_eq!(reader.read_piece(0).await?, bytes[..65536]);
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        sqlx::query("UPDATE rooms SET room_json=room_json").execute(&store.pool),
    )
    .await??;
    assert_eq!(reader.read_piece(1).await?, bytes[65536..]);
    sqlx::query("DELETE FROM user_profiles")
        .execute(&store.pool)
        .await?;
    assert!(reader.read_piece(1).await.is_err());
    Ok(())
}
