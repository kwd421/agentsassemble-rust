use std::io::Cursor;

use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, RoomSettings, public_settings,
};
use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
use sqlx::Row;

use crate::{LocalRoomManagerAuthority, PersistenceError, SqliteStore};

#[tokio::test]
async fn pending_room_asset_is_canonical_private_custody_until_expiry() {
    let (store, authority, _principal) = fixture().await;
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
    let (store, authority, _principal) = fixture().await;
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

#[tokio::test]
async fn settings_bind_replace_clear_and_rollback_room_owned_assets_atomically() {
    let (store, authority, principal) = fixture().await;
    let shared = store
        .store_pending_room_appearance_asset(&authority, "shared.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store shared appearance: {error}"));
    let initial = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("initial appearance revision: {error}"));
    let first = update_appearance(
        &store,
        &principal,
        "appearance-bind-shared",
        &initial.settings_revision,
        serde_json::json!({
            "banner_image_url": shared.url,
            "icon_image_url": shared.url,
            "banner_preset": "custom"
        }),
    )
    .await
    .unwrap_or_else(|error| panic!("bind shared appearance: {error}"));
    assert_bound(&store, &shared.id).await;

    let replacement = store
        .store_pending_room_appearance_asset(
            &authority,
            "replacement.png",
            "image/png",
            valid_png(),
        )
        .await
        .unwrap_or_else(|error| panic!("store replacement appearance: {error}"));
    let first_revision = result_revision(&first);
    let second = update_appearance(
        &store,
        &principal,
        "appearance-replace-banner",
        first_revision,
        serde_json::json!({"banner_image_url": replacement.url}),
    )
    .await
    .unwrap_or_else(|error| panic!("replace banner appearance: {error}"));
    assert_bound(&store, &shared.id).await;
    assert_bound(&store, &replacement.id).await;

    let second_revision = result_revision(&second);
    update_appearance(
        &store,
        &principal,
        "appearance-clear-icon",
        second_revision,
        serde_json::json!({"icon_image_url": ""}),
    )
    .await
    .unwrap_or_else(|error| panic!("clear icon appearance: {error}"));
    assert_missing(&store, &shared.id).await;
    assert_bound(&store, &replacement.id).await;

    let rollback = store
        .store_pending_room_appearance_asset(&authority, "rollback.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store rollback appearance: {error}"));
    sqlx::query(
        "CREATE TRIGGER reject_appearance_event BEFORE INSERT ON room_events WHEN json_extract(NEW.event_json, '$.type') = 'room_settings_updated' BEGIN SELECT RAISE(ABORT, 'injected appearance event failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("install appearance rollback trigger: {error}"));
    let current = stored_settings(&store).await;
    let current_revision = public_settings(&current)
        .unwrap_or_else(|error| panic!("rollback appearance revision: {error}"))
        .settings_revision;
    assert!(matches!(
        update_appearance(
            &store,
            &principal,
            "appearance-rollback",
            &current_revision,
            serde_json::json!({"banner_image_url": rollback.url}),
        )
        .await,
        Err(PersistenceError::Database(_))
    ));
    assert_bound(&store, &replacement.id).await;
    assert_pending(&store, &rollback.id).await;
    assert_eq!(stored_settings(&store).await, current);
}

#[tokio::test]
async fn settings_reject_expired_pending_without_partial_reference_or_promotion() {
    let (store, authority, principal) = fixture().await;
    let expired = store
        .store_pending_room_appearance_asset(&authority, "expired.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store expiring appearance: {error}"));
    sqlx::query("UPDATE room_appearance_assets SET expires_at = 0 WHERE asset_id = ?")
        .bind(&expired.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expire room appearance: {error}"));
    let initial = RoomSettings::defaults("General");
    let revision = public_settings(&initial)
        .unwrap_or_else(|error| panic!("expired appearance revision: {error}"))
        .settings_revision;

    assert_rejected_code(
        update_appearance(
            &store,
            &principal,
            "appearance-expired",
            &revision,
            serde_json::json!({"icon_image_url": expired.url}),
        )
        .await,
        "appearance_asset_missing",
    );
    assert_eq!(stored_settings(&store).await, initial);
    assert_pending(&store, &expired.id).await;
}

#[tokio::test]
async fn bound_read_requires_current_membership_reference_and_integral_bytes() {
    let (store, authority, principal) = fixture().await;
    let stored = store
        .store_pending_room_appearance_asset(&authority, "bound.png", "image/png", valid_png())
        .await
        .unwrap_or_else(|error| panic!("store bound-read appearance: {error}"));
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("bound-read appearance revision: {error}"))
        .settings_revision;
    update_appearance(
        &store,
        &principal,
        "appearance-bound-read",
        &revision,
        serde_json::json!({"icon_image_url": stored.url}),
    )
    .await
    .unwrap_or_else(|error| panic!("bind readable appearance: {error}"));

    let asset = store
        .bound_room_appearance_asset(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &stored.id,
        )
        .await
        .unwrap_or_else(|error| panic!("read bound appearance: {error}"));
    assert_eq!(&asset.content[..8], b"\x89PNG\r\n\x1a\n");

    sqlx::query("UPDATE room_appearance_assets SET size = size + 1 WHERE asset_id = ?")
        .bind(&stored.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("corrupt bound appearance size: {error}"));
    assert_rejected_code(
        store
            .bound_room_appearance_asset(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &stored.id,
            )
            .await,
        "invalid_state",
    );
    sqlx::query("UPDATE room_appearance_assets SET size = length(content) WHERE asset_id = ?")
        .bind(&stored.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("restore bound appearance size: {error}"));

    let defaults = serde_json::to_string(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("encode unreferenced appearance settings: {error}"));
    sqlx::query("UPDATE rooms SET settings_json = ? WHERE room_id = 'general'")
        .bind(defaults)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("orphan bound appearance: {error}"));
    assert_rejected_code(
        store
            .bound_room_appearance_asset(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &stored.id,
            )
            .await,
        "appearance_asset_missing",
    );
}

async fn fixture() -> (
    SqliteStore,
    LocalRoomManagerAuthority,
    AuthenticatedPrincipal,
) {
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
    (store, authority, principal)
}

async fn update_appearance(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    revision: &str,
    appearance: serde_json::Value,
) -> Result<crate::CommandOutcome, PersistenceError> {
    store
        .execute_room_settings_update(
            principal,
            request_id,
            &serde_json::json!({
                "expected_revision": revision,
                "appearance": appearance,
            }),
        )
        .await
}

fn result_revision(outcome: &crate::CommandOutcome) -> &str {
    outcome
        .result
        .pointer("/room_settings/settings_revision")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("appearance result revision"))
}

async fn stored_settings(store: &SqliteStore) -> RoomSettings {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT settings_json FROM rooms WHERE room_id = 'general'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read stored appearance settings: {error}"));
    serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode stored appearance settings: {error}"))
}

async fn assert_bound(store: &SqliteStore, asset_id: &str) {
    let row = sqlx::query(
        "SELECT pending_owner_user_id, state, expires_at FROM room_appearance_assets WHERE asset_id = ?",
    )
    .bind(asset_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read bound appearance {asset_id}: {error}"));
    assert_eq!(row.get::<String, _>("state"), "bound");
    assert!(
        row.get::<Option<String>, _>("pending_owner_user_id")
            .is_none()
    );
    assert!(row.get::<Option<i64>, _>("expires_at").is_none());
}

async fn assert_pending(store: &SqliteStore, asset_id: &str) {
    let state = sqlx::query_scalar::<_, String>(
        "SELECT state FROM room_appearance_assets WHERE asset_id = ?",
    )
    .bind(asset_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read pending appearance {asset_id}: {error}"));
    assert_eq!(state, "pending");
}

async fn assert_missing(store: &SqliteStore, asset_id: &str) {
    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_appearance_assets WHERE asset_id = ?",
    )
    .bind(asset_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("count missing appearance {asset_id}: {error}"));
    assert_eq!(count, 0);
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
