use std::io::Cursor;

use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use sqlx::Row;

use crate::{LocalRoomManagerAuthority, PersistenceError, SqliteStore};

#[tokio::test]
async fn pending_room_asset_is_canonical_private_custody_until_expiry() {
    let (store, authority) = fixture().await;
    let stored = store
        .store_pending_room_appearance_asset(&authority, "banner.webp", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store room appearance: {error}"));
    assert!(stored.id.starts_with("ra_"));
    assert_eq!(stored.id.len(), 35);
    assert_eq!(stored.url, format!("/api/attachments/{}?view=1", stored.id));
    assert_eq!(stored.content_type, "image/png");
    assert!(
        std::path::Path::new(&stored.filename)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
    );

    let row = sqlx::query(
        "SELECT room_id, pending_owner_user_id, state, expires_at FROM room_appearance_assets WHERE asset_id = ?",
    )
    .bind(&stored.id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read pending custody: {error}"));
    assert_eq!(row.get::<String, _>("room_id"), "general");
    assert_eq!(
        row.get::<Option<String>, _>("pending_owner_user_id")
            .as_deref(),
        Some(LOCAL_OPERATOR_USER_ID)
    );
    assert_eq!(row.get::<String, _>("state"), "pending");
    assert!(row.get::<Option<i64>, _>("expires_at").is_some());

    let preview = store
        .pending_room_appearance_asset(&authority, &stored.id)
        .await
        .unwrap_or_else(|error| panic!("preview pending appearance: {error}"));
    assert_eq!(preview.metadata, stored);
    assert_eq!(&preview.content[..8], b"\x89PNG\r\n\x1a\n");

    sqlx::query("UPDATE room_appearance_assets SET expires_at = 0 WHERE asset_id = ?")
        .bind(&stored.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expire pending appearance: {error}"));
    assert_rejected_code(
        store
            .pending_room_appearance_asset(&authority, &stored.id)
            .await,
        "appearance_asset_missing",
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_appearance_assets WHERE asset_id = ?",
        )
        .bind(&stored.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count expired appearance: {error}")),
        0
    );
}

#[tokio::test]
async fn pending_room_asset_revalidates_exact_manager_and_shared_raster_safety() {
    let (store, authority) = fixture().await;
    assert_rejected_code(
        store
            .store_pending_room_appearance_asset(
                &authority,
                "active.html",
                "image/png",
                b"<html>active</html>".to_vec(),
            )
            .await,
        "attachment_invalid_image",
    );

    let stored = store
        .store_pending_room_appearance_asset(&authority, "icon.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store valid appearance: {error}"));
    let mut stale = authority;
    stale.room_uid = uuid::Uuid::new_v4();
    assert_rejected_code(
        store
            .pending_room_appearance_asset(&stale, &stored.id)
            .await,
        "room_authority_changed",
    );
    assert_rejected_code(
        store
            .pending_room_appearance_asset(&stale, "ra_00000000000000000000000000000000")
            .await,
        "room_authority_changed",
    );
    assert_rejected_code(
        store
            .pending_room_appearance_asset(&stale, "profile_avatar")
            .await,
        "appearance_asset_missing",
    );
}

async fn fixture() -> (SqliteStore, LocalRoomManagerAuthority) {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open room appearance fixture: {error}"));
    store
        .bootstrap_local_authority("bc57f419-c3a4-4778-a98e-48f2542c4108", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap room appearance fixture: {error}"));
    store
        .create_room_for_local_operator(
            "b73ab311-234d-473b-80d1-03faad588412",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room appearance fixture: {error}"));
    let authority = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize room appearance manager: {error}"));
    (store, authority)
}

fn valid_png() -> Vec<u8> {
    let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(4, 4, Rgba([20, 40, 60, 255])));
    let mut encoded = Cursor::new(Vec::new());
    image
        .write_to(&mut encoded, ImageFormat::Png)
        .unwrap_or_else(|error| panic!("encode PNG fixture: {error}"));
    encoded.into_inner()
}

fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &'static str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected}, got {error}"),
        Ok(_) => panic!("expected {expected}, got success"),
    }
}
