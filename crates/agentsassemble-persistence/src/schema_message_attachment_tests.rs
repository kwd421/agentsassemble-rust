use agentsassemble_domain::{MAX_RASTER_BYTES, is_message_attachment_id};

use super::tests::installed_schema;

async fn seed_message_authority(pool: &sqlx::SqlitePool) {
    sqlx::query(
        "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
    )
    .execute(pool)
    .await
    .unwrap_or_else(|error| panic!("insert room: {error}"));
    for (user_id, participant_id) in [("user-1", "participant-1"), ("user-2", "participant-2")] {
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, '{}')",
        )
        .bind(user_id)
        .bind(participant_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert profile {user_id}: {error}"));
    }
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES ('general', 1, '{}')")
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert message event: {error}"));
}

#[tokio::test]
async fn message_attachment_ids_match_the_domain_namespace() {
    let pool = installed_schema().await;
    seed_message_authority(&pool).await;
    let candidates = [
        format!("ma_{}", "a".repeat(32)),
        format!("ma_{}", "A".repeat(32)),
        format!("ma_{}", "g".repeat(32)),
        format!("ma_{}", "0".repeat(31)),
        format!("mx_{}", "0".repeat(32)),
        format!("ma_{}\0x", "a".repeat(32)),
    ];

    for (index, attachment_id) in candidates.into_iter().enumerate() {
        let accepted = sqlx::query(
            "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, 'general', 'user-1', NULL, 'file.bin', 'application/octet-stream', X'00', 1, 0, 100, 'pending', ?)",
        )
        .bind(&attachment_id)
        .bind(200 + i64::try_from(index).unwrap_or(0))
        .execute(&pool)
        .await
        .is_ok();
        assert_eq!(
            accepted,
            is_message_attachment_id(&attachment_id),
            "installed schema disagreed with the domain for {attachment_id:?}"
        );
    }
}

#[tokio::test]
async fn pending_and_bound_message_custody_have_one_exact_owner() {
    let pool = installed_schema().await;
    seed_message_authority(&pool).await;
    let attachment_id = "ma_00000000000000000000000000000000";
    sqlx::query(
        "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, 'general', 'user-1', NULL, 'file.bin', 'application/octet-stream', X'00', 1, 0, 100, 'pending', 200)",
    )
    .bind(attachment_id)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert pending attachment: {error}"));

    for statement in [
        "UPDATE room_message_attachments SET pending_owner_user_id = NULL WHERE attachment_id = ?",
        "UPDATE room_message_attachments SET event_seq = 1 WHERE attachment_id = ?",
        "UPDATE room_message_attachments SET expires_at = NULL WHERE attachment_id = ?",
    ] {
        assert!(
            sqlx::query(statement)
                .bind(attachment_id)
                .execute(&pool)
                .await
                .is_err(),
            "accepted split pending custody: {statement}"
        );
    }
    sqlx::query(
        "UPDATE room_message_attachments SET pending_owner_user_id = NULL, event_seq = 1, state = 'bound', expires_at = NULL WHERE attachment_id = ?",
    )
    .bind(attachment_id)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("bind attachment to event: {error}"));

    let pending_id = "ma_ffffffffffffffffffffffffffffffff";
    sqlx::query(
        "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, 'general', 'user-2', NULL, 'pending.bin', 'application/octet-stream', X'00', 1, 0, 100, 'pending', 200)",
    )
    .bind(pending_id)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert second pending attachment: {error}"));
    sqlx::query("DELETE FROM user_profiles WHERE user_id = 'user-2'")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete pending uploader: {error}"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(pending_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read pending attachment: {error}")),
        0
    );

    sqlx::query("DELETE FROM user_profiles WHERE user_id = 'user-1'")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete former uploader: {error}"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(attachment_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read bound attachment: {error}")),
        1
    );
    sqlx::query("DELETE FROM room_events WHERE room_id = 'general' AND seq = 1")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete owning event: {error}"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(attachment_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read deleted attachment: {error}")),
        0
    );
}

#[tokio::test]
async fn message_attachment_bytes_and_event_reference_are_integral() {
    let pool = installed_schema().await;
    seed_message_authority(&pool).await;
    let byte_limit = i64::try_from(MAX_RASTER_BYTES)
        .unwrap_or_else(|error| panic!("attachment byte limit: {error}"));
    let at_limit_id = "ma_30000000000000000000000000000000";
    sqlx::query(
        "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, 'general', 'user-1', NULL, 'limit.bin', 'application/octet-stream', zeroblob(?), ?, 0, 100, 'pending', 200)",
    )
    .bind(at_limit_id)
    .bind(byte_limit)
    .bind(byte_limit)
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert attachment at byte limit: {error}"));
    assert!(
        sqlx::query(
            "UPDATE room_message_attachments SET content = zeroblob(?), size = ? WHERE attachment_id = ?",
        )
        .bind(byte_limit + 1)
        .bind(byte_limit + 1)
        .bind(at_limit_id)
        .execute(&pool)
        .await
        .is_err(),
        "accepted the attachment byte limit plus one"
    );

    for (attachment_id, content, size, event_seq) in [
        (
            "ma_10000000000000000000000000000000",
            vec![0_u8],
            2_i64,
            None,
        ),
        (
            "ma_20000000000000000000000000000000",
            vec![0_u8],
            1_i64,
            Some(2_i64),
        ),
    ] {
        assert!(
            sqlx::query(
                "INSERT INTO room_message_attachments(attachment_id, room_id, pending_owner_user_id, event_seq, filename, content_type, content, size, is_safe_image, created_at, state, expires_at) VALUES (?, 'general', NULL, ?, 'file.bin', 'application/octet-stream', ?, ?, 0, 100, 'bound', NULL)",
            )
            .bind(attachment_id)
            .bind(event_seq)
            .bind(content)
            .bind(size)
            .execute(&pool)
            .await
            .is_err(),
            "accepted non-integral attachment {attachment_id}"
        );
    }
}
