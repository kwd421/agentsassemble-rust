use super::tests::installed_schema;
use crate::raster_assets::MAX_RASTER_BYTES;

async fn seed_asset_authority(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
    )
    .execute(pool)
    .await
    .unwrap_or_else(|error| panic!("insert room: {error}"));
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES ('user-1', 'participant-1', '{}')",
    )
    .execute(pool)
    .await
    .unwrap_or_else(|error| panic!("insert profile: {error}"));
}

#[tokio::test]
async fn schema_item_limits_match_the_runtime_raster_owner() {
    let pool = installed_schema().await;
    seed_asset_authority(&pool).await;
    let limit =
        i64::try_from(MAX_RASTER_BYTES).unwrap_or_else(|error| panic!("raster limit: {error}"));

    sqlx::query(
        "INSERT INTO profile_avatar_assets(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('profile-limit', 'user-1', 'avatar.png', 'image/png', X'00', ?, '2026-08-26T00:00:00Z', 'pending', 1)",
    )
    .bind(limit)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert profile item at limit: {error}"));
    sqlx::query(
        "INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) VALUES ('prejoin-limit', 'general', ?, ?, 'avatar.png', 'image/png', X'00', ?, '2026-08-26T00:00:00Z', 1)",
    )
    .bind(vec![1; 32])
    .bind(vec![2; 32])
    .bind(limit)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert prejoin item at limit: {error}"));
    sqlx::query(
        "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_ffffffffffffffffffffffffffffffff', 'general', 'user-1', 'avatar.png', 'image/png', X'00', ?, '2026-08-26T00:00:00Z', 'pending', 1)",
    )
    .bind(limit)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert room item at limit: {error}"));

    for table in [
        "profile_avatar_assets",
        "prejoin_avatar_assets",
        "room_appearance_assets",
    ] {
        let query = format!("UPDATE {table} SET size = ?");
        assert!(
            sqlx::query(sqlx::AssertSqlSafe(query))
                .bind(limit + 1)
                .execute(&pool)
                .await
                .is_err(),
            "{table} accepted the runtime raster limit plus one"
        );
    }
}

#[tokio::test]
async fn profile_schema_owns_exactly_one_pending_and_one_current_avatar() {
    let pool = installed_schema().await;
    seed_asset_authority(&pool).await;
    for (attachment_id, state, expires_at) in [
        ("pending-1", "pending", Some(1)),
        ("current-1", "current", None),
    ] {
        sqlx::query(
            "INSERT INTO profile_avatar_assets(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, 'user-1', 'avatar.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', ?, ?)",
        )
        .bind(attachment_id)
        .bind(state)
        .bind(expires_at)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert {state} profile avatar: {error}"));
    }
    for (attachment_id, state, expires_at) in [
        ("pending-2", "pending", Some(2)),
        ("current-2", "current", None),
    ] {
        assert!(
            sqlx::query(
                "INSERT INTO profile_avatar_assets(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES (?, 'user-1', 'avatar.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', ?, ?)",
            )
            .bind(attachment_id)
            .bind(state)
            .bind(expires_at)
            .execute(&pool)
            .await
            .is_err()
        );
    }
    assert!(
        sqlx::query(
            "INSERT INTO profile_avatar_assets(attachment_id, owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('invalid-current', 'user-1', 'avatar.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'current', 2)",
        )
        .execute(&pool)
        .await
        .is_err()
    );
}

#[tokio::test]
async fn prejoin_schema_has_one_expiring_asset_per_custody() {
    let pool = installed_schema().await;
    seed_asset_authority(&pool).await;
    sqlx::query(
        "INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) VALUES ('prejoin-1', 'general', ?, ?, 'avatar.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 1)",
    )
    .bind(vec![1; 32])
    .bind(vec![2; 32])
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert prejoin avatar: {error}"));
    assert!(
        sqlx::query(
            "INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) VALUES ('prejoin-2', 'general', ?, ?, 'avatar.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 2)",
        )
        .bind(vec![1; 32])
        .bind(vec![3; 32])
        .execute(&pool)
        .await
        .is_err()
    );
    let columns = sqlx::query_scalar::<_, String>(
        "SELECT name FROM pragma_table_info('prejoin_avatar_assets')",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|error| panic!("read prejoin columns: {error}"));
    assert!(!columns.iter().any(|column| column == "state"));
}

#[tokio::test]
async fn room_asset_custody_moves_from_uploader_to_room_in_the_schema() {
    let pool = installed_schema().await;
    seed_asset_authority(&pool).await;
    sqlx::query(
        "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_00000000000000000000000000000000', 'general', 'user-1', 'pending.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'pending', 1)",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert pending asset: {error}"));
    sqlx::query(
        "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_11111111111111111111111111111111', 'general', NULL, 'bound.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'bound', NULL)",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert bound asset: {error}"));
    assert!(
        sqlx::query(
            "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_22222222222222222222222222222222', 'general', NULL, 'invalid.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'pending', NULL)",
        )
        .execute(&pool)
        .await
        .is_err()
    );
    sqlx::query("DELETE FROM user_profiles WHERE user_id = 'user-1'")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete uploader: {error}"));
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT asset_id FROM room_appearance_assets ORDER BY asset_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|error| panic!("read remaining assets: {error}")),
        vec!["ra_11111111111111111111111111111111".to_owned()]
    );
    let columns = sqlx::query_scalar::<_, String>(
        "SELECT name FROM pragma_table_info('room_appearance_assets')",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|error| panic!("read room asset columns: {error}"));
    assert!(!columns.iter().any(|column| column == "created_by_user_id"));
}
