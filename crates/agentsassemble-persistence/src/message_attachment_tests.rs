use std::io::Cursor;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, Participant,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use sqlx::Row;

use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn pending_message_upload_preserves_bytes_and_classifies_only_verified_rasters() {
    let (store, principal) = fixture().await;
    let png = valid_png();
    let image = store
        .store_message_attachment(
            &principal,
            "../folder\\safe.png",
            " IMAGE/PNG; charset=binary ",
            png.clone(),
        )
        .await
        .unwrap_or_else(|error| panic!("store safe message image: {error}"));
    assert_eq!(image.filename, "safe.png");
    assert_eq!(image.content_type, "image/png");
    assert_eq!(image.size, png.len());
    assert!(image.is_image);
    assert_eq!(image.url, format!("/api/attachments/{}?view=1", image.id));

    let stored = sqlx::query(
        "SELECT room_id, pending_owner_user_id, content, size, is_safe_image, created_at, expires_at FROM room_message_attachments WHERE attachment_id = ?",
    )
    .bind(&image.id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read safe message image: {error}"));
    assert_eq!(stored.get::<String, _>("room_id"), "general");
    assert_eq!(
        stored.get::<String, _>("pending_owner_user_id"),
        LOCAL_OPERATOR_USER_ID
    );
    assert_eq!(stored.get::<Vec<u8>, _>("content"), png);
    assert_eq!(stored.get::<i64, _>("is_safe_image"), 1);
    assert!(stored.get::<i64, _>("expires_at") > stored.get::<i64, _>("created_at"));

    let active = b"<html><script>active()</script></html>".to_vec();
    let download = store
        .store_message_attachment(&principal, "../active.html", "text/html", active.clone())
        .await
        .unwrap_or_else(|error| panic!("store download-only message file: {error}"));
    assert_eq!(download.filename, "active.html");
    assert_eq!(download.content_type, "text/html");
    assert!(!download.is_image);
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT content FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(&download.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read preserved download bytes: {error}")),
        active
    );

    let opaque_png = store
        .store_message_attachment(
            &principal,
            "opaque.png",
            "application/octet-stream",
            valid_png(),
        )
        .await
        .unwrap_or_else(|error| panic!("store explicitly opaque raster: {error}"));
    assert!(!opaque_png.is_image);

    assert_rejected_code(
        store
            .store_message_attachment(&principal, "mismatch.jpg", "image/jpeg", valid_png())
            .await,
        "attachment_type_mismatch",
    );
}

#[tokio::test]
async fn upload_revalidates_room_write_authority_without_partial_storage() {
    let (store, principal) = fixture().await;
    let mut read_only = principal.clone();
    read_only.invite_scope = InviteScope::ReadOnly;
    read_only.capabilities =
        CapabilitySet::for_principal(ClientKind::Browser, InviteScope::ReadOnly, false);
    assert_rejected_code(
        store
            .store_message_attachment(&read_only, "denied.txt", "text/plain", b"denied".to_vec())
            .await,
        "permission_denied",
    );

    set_operator_muted(&store, true).await;
    assert_rejected_code(
        store
            .store_message_attachment(&principal, "muted.txt", "text/plain", b"muted".to_vec())
            .await,
        "muted",
    );
    assert_eq!(count_message_attachments(&store).await, 0);
}

#[tokio::test]
async fn upload_cleans_only_expired_message_pending_rows() {
    let (store, principal) = fixture().await;
    let profile = store
        .store_profile_attachment(&principal, "profile.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store expired profile fixture: {error}"));
    sqlx::query("UPDATE profile_avatar_assets SET expires_at = 0 WHERE attachment_id = ?")
        .bind(&profile.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expire profile fixture: {error}"));

    let expired = store
        .store_message_attachment(&principal, "expired.txt", "text/plain", b"expired".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store expired message fixture: {error}"));
    sqlx::query(
        "UPDATE room_message_attachments SET created_at = 1, expires_at = 2 WHERE attachment_id = ?",
    )
        .bind(&expired.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expire message fixture: {error}"));

    let fresh = store
        .store_message_attachment(
            &principal,
            "fresh.txt",
            "invalid content type",
            b"fresh".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store fresh message fixture: {error}"));
    assert_eq!(fresh.content_type, "text/plain");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(&expired.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count expired message fixture: {error}")),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM profile_avatar_assets WHERE attachment_id = ?",
        )
        .bind(&profile.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count untouched profile fixture: {error}")),
        1
    );
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open message attachment fixture: {error}"));
    store
        .bootstrap_local_authority("1468c3c6-a7f5-4311-b262-a4e492db1032", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap message attachment fixture: {error}"));
    store
        .create_room_for_local_operator(
            "6f218ed3-8fe3-4487-86e4-3120c41fdd9e",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create message attachment room: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal)
}

async fn set_operator_muted(store: &SqliteStore, muted: bool) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read operator participant: {error}"));
    let mut participant: Participant = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode operator participant: {error}"));
    participant.muted = muted;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode operator participant: {error}")),
    )
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("mute operator participant: {error}"));
}

async fn count_message_attachments(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_message_attachments")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count message attachments: {error}"))
}

fn valid_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([20, 40, 60, 255])));
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap_or_else(|error| panic!("encode message image fixture: {error}"));
    encoded.into_inner()
}

fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected} rejection, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
}
