use std::collections::BTreeSet;

use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};

use super::{product_tables, statements, tables};

pub(crate) async fn installed_schema() -> SqlitePool {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open in-memory schema database: {error}"));
    sqlx::query("PRAGMA foreign_keys = ON")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("enable foreign keys: {error}"));
    for statement in statements() {
        sqlx::query(statement)
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("install schema statement: {error}"));
    }
    pool
}

#[tokio::test]
async fn installed_table_set_and_product_inventory_share_one_descriptor_owner() {
    let pool = installed_schema().await;
    let actual = sqlx::query(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
    )
    .fetch_all(&pool)
    .await
    .unwrap_or_else(|error| panic!("read installed tables: {error}"))
    .into_iter()
    .map(|row| row.get::<String, _>("name"))
    .collect::<BTreeSet<_>>();
    let declared = tables()
        .map(|table| table.name.to_owned())
        .chain(
            crate::schema_message_search::FTS_SHADOW_TABLES
                .iter()
                .map(|name| (*name).to_owned()),
        )
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, declared);

    let product = product_tables()
        .map(|table| table.name)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        product.len(),
        tables().filter(|table| !table.infrastructure).count()
    );
    for table in product_tables() {
        let sql = format!("SELECT EXISTS(SELECT 1 FROM {} LIMIT 1)", table.name);
        assert!(
            !sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(sql))
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|error| panic!("query product table {}: {error}", table.name))
        );
    }
}

#[tokio::test]
async fn fixed_authority_bytes_require_blob_storage() {
    let pool = installed_schema().await;
    let text_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    assert!(
        sqlx::query(
            "INSERT INTO runtime_host_identity(singleton, server_id, public_key, session_hmac_key_fingerprint) VALUES (1, 'server-text', ?, ?)",
        )
        .bind(text_key)
        .bind(vec![0xBB; 32])
        .execute(&pool)
        .await
        .is_err()
    );
    sqlx::query(
        "INSERT INTO runtime_host_identity(singleton, server_id, public_key, session_hmac_key_fingerprint) VALUES (1, 'server-blob', ?, ?)",
    )
    .bind(vec![0xAA; 32])
    .bind(vec![0xBB; 32])
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert exact BLOB authority: {error}"));
}

pub(super) async fn seed_human_authorities(pool: &SqlitePool) {
    for room_id in ["room-a", "room-b"] {
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, '{}', '{}')",
        )
        .bind(room_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert room {room_id}: {error}"));
    }
    for (user_id, participant_id) in [("user-a", "participant-a"), ("user-b", "participant-b")]
    {
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, '{}')",
        )
        .bind(user_id)
        .bind(participant_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert profile {user_id}: {error}"));
    }
    for (room_id, participant_id) in [
        ("room-a", "participant-a"),
        ("room-a", "participant-b"),
        ("room-b", "participant-b"),
    ] {
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, '{}')",
        )
        .bind(room_id)
        .bind(participant_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| {
            panic!("insert participant {room_id}/{participant_id}: {error}")
        });
    }
    for (invite_id, signed_fingerprint, join_fingerprint, scope, max_uses) in [
        (
            "1111111111111111",
            vec![0x11; 32],
            vec![0x31; 32],
            "read_only",
            1_i64,
        ),
        (
            "2222222222222222",
            vec![0x22; 32],
            vec![0x32; 32],
            "read_write",
            5_i64,
        ),
    ] {
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'room-a', 'participant-a', 'Guest', ?, ?, 0, 200, 0, 'user-a', 100)",
        )
        .bind(invite_id)
        .bind(signed_fingerprint)
        .bind(join_fingerprint)
        .bind(scope)
        .bind(max_uses)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert invite {invite_id}: {error}"));
    }
    for (fingerprint, user_id) in [(vec![0x71; 32], "user-a"), (vec![0x72; 32], "user-b")] {
        sqlx::query(
            "INSERT INTO human_device_credentials(credential_fingerprint, user_id, created_at) VALUES (?, ?, 100)",
        )
        .bind(fingerprint)
        .bind(user_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert credential for {user_id}: {error}"));
    }
}

pub(super) struct SessionAuthority<'a> {
    pub(super) marker: u8,
    pub(super) invite_id: &'a str,
    pub(super) key_kind: &'a str,
    pub(super) room_id: &'a str,
    pub(super) user_id: &'a str,
    pub(super) participant_id: &'a str,
    pub(super) invite_scope: &'a str,
    pub(super) reusable_identity: Option<Vec<u8>>,
}

pub(super) async fn insert_human_session(
    pool: &SqlitePool,
    session: SessionAuthority<'_>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO human_room_sessions(admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, ?, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', ?, ?, ?, ?, ?, ?, 'browser', ?, ?, ?, '{}', 100, 200, 'ended')",
    )
    .bind(vec![session.marker; 32])
    .bind(session.key_kind)
    .bind(session.invite_id)
    .bind(vec![0x31; 32])
    .bind(vec![session.marker.wrapping_add(0x40); 32])
    .bind(session.room_id)
    .bind(session.user_id)
    .bind(session.participant_id)
    .bind(session.invite_scope)
    .bind(
        session
            .reusable_identity
            .clone()
            .unwrap_or_else(|| vec![session.marker.wrapping_add(0x20); 32]),
    )
    .bind(session.reusable_identity)
    .execute(pool)
    .await
    .map(|_| ())
}

fn invalid_human_session_authorities<'a>(
    one_use: &'a str,
    reusable: &'a str,
) -> [SessionAuthority<'a>; 6] {
    [
        SessionAuthority {
            marker: 1,
            invite_id: reusable,
            key_kind: "one_use",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_write",
            reusable_identity: None,
        },
        SessionAuthority {
            marker: 2,
            invite_id: one_use,
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_only",
            reusable_identity: Some(vec![0x71; 32]),
        },
        SessionAuthority {
            marker: 3,
            invite_id: reusable,
            key_kind: "reusable",
            room_id: "room-b",
            user_id: "user-b",
            participant_id: "participant-b",
            invite_scope: "read_write",
            reusable_identity: Some(vec![0x72; 32]),
        },
        SessionAuthority {
            marker: 4,
            invite_id: reusable,
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_only",
            reusable_identity: Some(vec![0x71; 32]),
        },
        SessionAuthority {
            marker: 5,
            invite_id: reusable,
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-b",
            invite_scope: "read_write",
            reusable_identity: Some(vec![0x71; 32]),
        },
        SessionAuthority {
            marker: 6,
            invite_id: reusable,
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_write",
            reusable_identity: Some(vec![0x72; 32]),
        },
    ]
}

#[tokio::test]
async fn human_sessions_require_exact_composite_authority() {
    let pool = installed_schema().await;
    seed_human_authorities(&pool).await;
    let one_use = "1111111111111111";
    let reusable = "2222222222222222";

    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT key_kind FROM room_invites WHERE invite_id = ?",
        )
        .bind(one_use)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read one-use classification: {error}")),
        "one_use"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT key_kind FROM room_invites WHERE invite_id = ?",
        )
        .bind(reusable)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read reusable classification: {error}")),
        "reusable"
    );

    for invalid in invalid_human_session_authorities(one_use, reusable) {
        assert!(insert_human_session(&pool, invalid).await.is_err());
    }

    insert_human_session(
        &pool,
        SessionAuthority {
            marker: 7,
            invite_id: one_use,
            key_kind: "one_use",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_only",
            reusable_identity: None,
        },
    )
    .await
    .unwrap_or_else(|error| panic!("insert valid one-use session: {error}"));
    insert_human_session(
        &pool,
        SessionAuthority {
            marker: 8,
            invite_id: reusable,
            key_kind: "reusable",
            room_id: "room-a",
            user_id: "user-a",
            participant_id: "participant-a",
            invite_scope: "read_write",
            reusable_identity: Some(vec![0x71; 32]),
        },
    )
    .await
    .unwrap_or_else(|error| panic!("insert valid reusable session: {error}"));

    let session_count =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|error| panic!("count admitted sessions: {error}"));
    assert_eq!(session_count, 2);
}

#[tokio::test]
async fn invite_schema_preserves_configured_values_and_key_kind() {
    let pool = installed_schema().await;
    sqlx::query(
        "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('room-a', '{}', '{}')",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert room: {error}"));
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES ('user-a', 'participant-a', '{}')",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert profile: {error}"));

    assert!(
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES ('aaaaaaaaaaaaaaaa', ?, ?, 'room-a', 'participant-a', 'Guest', 'read_write', 1, 0, 200, 0, 'user-a', 100)",
        )
        .bind(vec![0xBB; 32])
        .bind(vec![0xBC; 32])
        .execute(&pool)
        .await
        .is_err()
    );

    for (index, configured) in [0_i64, 1, 2, 3, 5, 128, 129].into_iter().enumerate() {
        let index = u8::try_from(index)
            .unwrap_or_else(|_| panic!("invite limit fixture index exceeds u8"));
        let accepted_marker = index + 1;
        let accepted_id = format!("{accepted_marker:02x}").repeat(8);
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'room-a', 'participant-a', 'Guest', 'read_write', ?, ?, 200, 0, 'user-a', 100)",
        )
        .bind(&accepted_id)
        .bind(vec![accepted_marker; 32])
        .bind(vec![index + 0x21; 32])
        .bind(configured)
        .bind(0_i64)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert configured invite {configured}: {error}"));

        let stored =
            sqlx::query("SELECT max_uses, key_kind FROM room_invites WHERE invite_id = ?")
                .bind(&accepted_id)
                .fetch_one(&pool)
                .await
                .unwrap_or_else(|error| panic!("read configured invite {configured}: {error}"));
        assert_eq!(stored.get::<i64, _>("max_uses"), configured);
        assert_eq!(
            stored.get::<String, _>("key_kind"),
            if configured == 1 {
                "one_use"
            } else {
                "reusable"
            }
        );
    }
}
