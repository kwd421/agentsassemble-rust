pub(crate) struct TableDefinition {
    pub name: &'static str,
    pub ddl: &'static str,
    pub infrastructure: bool,
}

pub(crate) const HOST_INITIALIZATION_DDL: &str = "CREATE TABLE IF NOT EXISTS runtime_host_initialization (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), nonce TEXT NOT NULL UNIQUE CHECK(length(nonce) = 36))";

const TABLES: &[TableDefinition] = &[
    TableDefinition {
        name: "runtime_metadata",
        ddl: "CREATE TABLE IF NOT EXISTS runtime_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        infrastructure: true,
    },
    TableDefinition {
        name: "runtime_host_initialization",
        ddl: HOST_INITIALIZATION_DDL,
        infrastructure: true,
    },
    TableDefinition {
        name: "local_bootstrap_authority",
        ddl: "CREATE TABLE IF NOT EXISTS local_bootstrap_authority (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), authority_lineage_id TEXT NOT NULL UNIQUE, state TEXT NOT NULL CHECK(state IN ('empty', 'initializing', 'complete')), request_id TEXT NOT NULL DEFAULT '', schema_revision INTEGER NOT NULL CHECK(schema_revision > 0), initialization_digest TEXT NOT NULL DEFAULT '', user_id TEXT NOT NULL DEFAULT '', participant_id TEXT NOT NULL DEFAULT '', result_json TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, completed_at TEXT)",
        infrastructure: true,
    },
    TableDefinition {
        name: "runtime_host_identity",
        ddl: "CREATE TABLE IF NOT EXISTS runtime_host_identity (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), server_id TEXT NOT NULL UNIQUE, public_key BLOB NOT NULL CHECK(typeof(public_key) = 'blob' AND length(public_key) = 32), session_hmac_key_fingerprint BLOB NOT NULL CHECK(typeof(session_hmac_key_fingerprint) = 'blob' AND length(session_hmac_key_fingerprint) = 32))",
        infrastructure: true,
    },
    TableDefinition {
        name: "rooms",
        ddl: "CREATE TABLE IF NOT EXISTS rooms (room_id TEXT PRIMARY KEY, room_json TEXT NOT NULL, settings_json TEXT NOT NULL)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_create_results",
        ddl: "CREATE TABLE IF NOT EXISTS room_create_results (principal_id TEXT NOT NULL, request_id TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(principal_id, request_id))",
        infrastructure: false,
    },
    TableDefinition {
        name: "participants",
        ddl: "CREATE TABLE IF NOT EXISTS participants (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, participant_json TEXT NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "user_profiles",
        ddl: "CREATE TABLE IF NOT EXISTS user_profiles (user_id TEXT PRIMARY KEY, participant_id TEXT NOT NULL UNIQUE, profile_json TEXT NOT NULL, UNIQUE(user_id, participant_id))",
        infrastructure: false,
    },
    TableDefinition {
        name: "profile_attachments",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS profile_attachments (",
            "attachment_id TEXT PRIMARY KEY, ",
            "owner_user_id TEXT, ",
            "admission_room_id TEXT, ",
            "admission_custody_fingerprint BLOB ",
            "CHECK(admission_custody_fingerprint IS NULL OR ",
            "(typeof(admission_custody_fingerprint) = 'blob' ",
            "AND length(admission_custody_fingerprint) = 32)), ",
            "invite_quota_fingerprint BLOB ",
            "CHECK(invite_quota_fingerprint IS NULL OR ",
            "(typeof(invite_quota_fingerprint) = 'blob' ",
            "AND length(invite_quota_fingerprint) = 32)), ",
            "filename TEXT NOT NULL, ",
            "content_type TEXT NOT NULL CHECK(content_type = 'image/png'), ",
            "content BLOB NOT NULL, ",
            "size INTEGER NOT NULL CHECK(size >= 0 AND size <= 10485760), ",
            "created_at TEXT NOT NULL, ",
            "state TEXT NOT NULL CHECK(state IN ('pending', 'bound', 'admission_pending')), ",
            "expires_at INTEGER, ",
            "CHECK(",
            "(state = 'pending' AND owner_user_id IS NOT NULL ",
            "AND admission_room_id IS NULL AND admission_custody_fingerprint IS NULL ",
            "AND invite_quota_fingerprint IS NULL AND expires_at IS NOT NULL) OR ",
            "(state = 'bound' AND owner_user_id IS NOT NULL ",
            "AND admission_custody_fingerprint IS NULL AND expires_at IS NULL AND ",
            "((admission_room_id IS NULL AND invite_quota_fingerprint IS NULL) OR ",
            "(admission_room_id IS NOT NULL AND length(admission_room_id) > 0 ",
            "AND invite_quota_fingerprint IS NOT NULL ",
            "AND length(invite_quota_fingerprint) = 32))) OR ",
            "(state = 'admission_pending' AND owner_user_id IS NULL ",
            "AND admission_room_id IS NOT NULL AND length(admission_room_id) > 0 ",
            "AND admission_custody_fingerprint IS NOT NULL ",
            "AND length(admission_custody_fingerprint) = 32 ",
            "AND invite_quota_fingerprint IS NOT NULL ",
            "AND length(invite_quota_fingerprint) = 32 AND expires_at IS NOT NULL)), ",
            "FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_user_preferences",
        ddl: "CREATE TABLE IF NOT EXISTS room_user_preferences (user_id TEXT NOT NULL, room_id TEXT NOT NULL, preferences_json TEXT NOT NULL, PRIMARY KEY(user_id, room_id), FOREIGN KEY(user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE, FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_invites",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS room_invites (",
            "invite_id TEXT PRIMARY KEY CHECK(",
            "typeof(invite_id) = 'text' AND length(invite_id) = 16 ",
            "AND invite_id = lower(invite_id) ",
            "AND invite_id NOT GLOB '*[^0-9a-f]*'), ",
            "signed_token_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(signed_token_fingerprint) = 'blob' ",
            "AND length(signed_token_fingerprint) = 32), ",
            "join_code_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(join_code_fingerprint) = 'blob' ",
            "AND length(join_code_fingerprint) = 32), ",
            "room_id TEXT NOT NULL, ",
            "base_participant_id TEXT NOT NULL CHECK(length(base_participant_id) > 0), ",
            "display_name TEXT NOT NULL CHECK(length(display_name) > 0), ",
            "invite_scope TEXT NOT NULL CHECK(invite_scope IN ('read_write', 'read_only')), ",
            "max_uses INTEGER NOT NULL CHECK(max_uses >= 0), ",
            "key_kind TEXT GENERATED ALWAYS AS (CASE ",
            "WHEN max_uses = 1 THEN 'one_use' ELSE 'reusable' END) STORED, ",
            "use_count INTEGER NOT NULL DEFAULT 0 CHECK(",
            "use_count >= 0 AND use_count <= CASE ",
            "WHEN max_uses = 1 THEN 1 ",
            "WHEN max_uses = 0 OR max_uses > 128 THEN 128 ",
            "ELSE max_uses END), ",
            "expires_at INTEGER NOT NULL, ",
            "revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)), ",
            "created_by_user_id TEXT NOT NULL, ",
            "created_at INTEGER NOT NULL, ",
            "CHECK(expires_at > created_at), ",
            "UNIQUE(invite_id, room_id, invite_scope, key_kind), ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(created_by_user_id) REFERENCES user_profiles(user_id))",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "human_device_credentials",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS human_device_credentials (",
            "credential_fingerprint BLOB PRIMARY KEY ",
            "CHECK(typeof(credential_fingerprint) = 'blob' ",
            "AND length(credential_fingerprint) = 32), ",
            "user_id TEXT NOT NULL UNIQUE, ",
            "created_at INTEGER NOT NULL, ",
            "UNIQUE(credential_fingerprint, user_id), ",
            "FOREIGN KEY(user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "human_room_sessions",
        ddl: concat!(
            "CREATE TABLE IF NOT EXISTS human_room_sessions (",
            "admission_key BLOB PRIMARY KEY ",
            "CHECK(typeof(admission_key) = 'blob' AND length(admission_key) = 32), ",
            "key_kind TEXT NOT NULL CHECK(key_kind IN ('one_use', 'reusable')), ",
            "first_request_id TEXT NOT NULL CHECK(length(first_request_id) = 36), ",
            "invite_id TEXT NOT NULL, ",
            "payload_hash BLOB NOT NULL ",
            "CHECK(typeof(payload_hash) = 'blob' AND length(payload_hash) = 32), ",
            "session_fingerprint BLOB NOT NULL UNIQUE ",
            "CHECK(typeof(session_fingerprint) = 'blob' AND length(session_fingerprint) = 32), ",
            "room_id TEXT NOT NULL, ",
            "user_id TEXT NOT NULL, ",
            "participant_id TEXT NOT NULL, ",
            "client_kind TEXT NOT NULL CHECK(client_kind = 'browser'), ",
            "invite_scope TEXT NOT NULL CHECK(invite_scope IN ('read_write', 'read_only')), ",
            "browser_credential_fingerprint BLOB NOT NULL ",
            "CHECK(typeof(browser_credential_fingerprint) = 'blob' ",
            "AND length(browser_credential_fingerprint) = 32), ",
            "reusable_identity_fingerprint BLOB ",
            "CHECK(reusable_identity_fingerprint IS NULL OR ",
            "(typeof(reusable_identity_fingerprint) = 'blob' ",
            "AND length(reusable_identity_fingerprint) = 32)), ",
            "result_json TEXT NOT NULL, ",
            "admitted_at INTEGER NOT NULL, ",
            "expires_at INTEGER NOT NULL, ",
            "state TEXT NOT NULL CHECK(state IN ('active', 'ended')), ",
            "CHECK(expires_at > admitted_at), ",
            "CHECK(",
            "(key_kind = 'one_use' AND reusable_identity_fingerprint IS NULL) OR ",
            "(key_kind = 'reusable' AND reusable_identity_fingerprint IS NOT NULL ",
            "AND length(reusable_identity_fingerprint) = 32)), ",
            "FOREIGN KEY(invite_id, room_id, invite_scope, key_kind) ",
            "REFERENCES room_invites(invite_id, room_id, invite_scope, key_kind) ",
            "ON DELETE CASCADE, ",
            "FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(user_id, participant_id) ",
            "REFERENCES user_profiles(user_id, participant_id) ON DELETE CASCADE, ",
            "FOREIGN KEY(reusable_identity_fingerprint, user_id) ",
            "REFERENCES human_device_credentials(credential_fingerprint, user_id), ",
            "FOREIGN KEY(room_id, participant_id) ",
            "REFERENCES participants(room_id, participant_id) ON DELETE CASCADE)",
        ),
        infrastructure: false,
    },
    TableDefinition {
        name: "room_appearance_assets",
        ddl: "CREATE TABLE IF NOT EXISTS room_appearance_assets (asset_id TEXT PRIMARY KEY CHECK(length(asset_id) = 35 AND substr(asset_id, 1, 3) = 'ra_'), room_id TEXT NOT NULL, pending_owner_user_id TEXT, created_by_user_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size > 0 AND size <= 10485760), created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'bound')), expires_at INTEGER, CHECK((state = 'pending' AND pending_owner_user_id IS NOT NULL AND expires_at IS NOT NULL) OR (state = 'bound' AND pending_owner_user_id IS NULL AND expires_at IS NULL)), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE, FOREIGN KEY(pending_owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "agent_sessions",
        ddl: "CREATE TABLE IF NOT EXISTS agent_sessions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, session_json TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "provider_turn_executions",
        ddl: "CREATE TABLE IF NOT EXISTS provider_turn_executions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_generation INTEGER NOT NULL CHECK(turn_generation > 0), execution_id TEXT NOT NULL, participant_id TEXT NOT NULL, turn_id TEXT NOT NULL, assignment_json TEXT NOT NULL, phase TEXT NOT NULL CHECK(phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required', 'completed', 'declined', 'failed', 'interrupted')), runtime_handle_id TEXT NOT NULL CHECK(length(runtime_handle_id) > 0), runtime_owner_id TEXT NOT NULL CHECK(length(runtime_owner_id) > 0), runtime_lease_token TEXT NOT NULL CHECK(length(runtime_lease_token) > 0), start_dispatch_nonce TEXT NOT NULL DEFAULT '', provider_turn_id TEXT NOT NULL DEFAULT '', requeue_finalized INTEGER NOT NULL DEFAULT 0 CHECK(requeue_finalized IN (0, 1)), created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(room_id, session_id, turn_generation), UNIQUE(room_id, execution_id), UNIQUE(room_id, session_id, turn_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE, FOREIGN KEY(room_id, participant_id) REFERENCES participants(room_id, participant_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "provider_turn_effects",
        ddl: "CREATE TABLE IF NOT EXISTS provider_turn_effects (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_generation INTEGER NOT NULL CHECK(turn_generation > 0), effect_id TEXT NOT NULL, effect_kind TEXT NOT NULL CHECK(effect_kind = 'interrupt'), phase TEXT NOT NULL CHECK(phase IN ('prepared', 'claimed', 'dispatching', 'issued_waiting_quiescence', 'interrupt_ambiguous', 'recovery_required', 'finalized')), claim_owner TEXT NOT NULL DEFAULT '', claim_expires_at INTEGER, dispatch_nonce TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, updated_at TEXT NOT NULL, PRIMARY KEY(room_id, effect_id), UNIQUE(room_id, session_id, turn_generation, effect_kind), FOREIGN KEY(room_id, session_id, turn_generation) REFERENCES provider_turn_executions(room_id, session_id, turn_generation) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "lifecycle_command_reservations",
        ddl: "CREATE TABLE IF NOT EXISTS lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, principal_json TEXT NOT NULL, payload_json TEXT NOT NULL, supervisor_generation TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'rejected')), phase TEXT NOT NULL DEFAULT 'lifecycle_prepared', prepared_result_json TEXT NOT NULL DEFAULT '{}', failure_code TEXT NOT NULL DEFAULT '', failure_message TEXT NOT NULL DEFAULT '', PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_events",
        ddl: "CREATE TABLE IF NOT EXISTS room_events (room_id TEXT NOT NULL, seq INTEGER NOT NULL CHECK(seq > 0), event_json TEXT NOT NULL, PRIMARY KEY(room_id, seq), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_turn_tool_results",
        ddl: "CREATE TABLE IF NOT EXISTS room_turn_tool_results (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL, result_id TEXT NOT NULL, event_seq INTEGER NOT NULL CHECK(event_seq > 0), PRIMARY KEY(room_id, result_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_write_budgets",
        ddl: "CREATE TABLE IF NOT EXISTS room_write_budgets (room_id TEXT NOT NULL, window_started_at INTEGER NOT NULL, command_count INTEGER NOT NULL CHECK(command_count >= 0), payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0), PRIMARY KEY(room_id, window_started_at), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "room_event_publication_cursors",
        ddl: "CREATE TABLE IF NOT EXISTS room_event_publication_cursors (room_id TEXT PRIMARY KEY, published_seq INTEGER NOT NULL CHECK(published_seq >= 0), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
    TableDefinition {
        name: "command_results",
        ddl: "CREATE TABLE IF NOT EXISTS command_results (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        infrastructure: false,
    },
];

const INDEXES: &[&str] = &[
    "CREATE INDEX IF NOT EXISTS profile_attachments_owner_idx ON profile_attachments(owner_user_id)",
    "CREATE UNIQUE INDEX IF NOT EXISTS profile_attachments_admission_custody_idx ON profile_attachments(admission_custody_fingerprint) WHERE state = 'admission_pending'",
    "CREATE INDEX IF NOT EXISTS profile_attachments_invite_quota_idx ON profile_attachments(invite_quota_fingerprint, expires_at) WHERE invite_quota_fingerprint IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS profile_attachments_admission_room_idx ON profile_attachments(admission_room_id, state, expires_at) WHERE admission_room_id IS NOT NULL",
    "CREATE INDEX IF NOT EXISTS room_invites_room_state_idx ON room_invites(room_id, revoked, expires_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS human_room_sessions_active_participant_idx ON human_room_sessions(room_id, participant_id) WHERE state = 'active'",
    "CREATE UNIQUE INDEX IF NOT EXISTS human_room_sessions_reusable_identity_idx ON human_room_sessions(invite_id, reusable_identity_fingerprint) WHERE key_kind = 'reusable'",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_live_idx ON human_room_sessions(state, expires_at)",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_room_live_idx ON human_room_sessions(room_id, state, expires_at)",
    "CREATE INDEX IF NOT EXISTS human_room_sessions_invite_state_idx ON human_room_sessions(invite_id, key_kind, state)",
    "CREATE INDEX IF NOT EXISTS room_appearance_assets_creator_idx ON room_appearance_assets(created_by_user_id)",
    "CREATE INDEX IF NOT EXISTS room_appearance_assets_pending_idx ON room_appearance_assets(pending_owner_user_id, expires_at) WHERE state = 'pending'",
    "CREATE INDEX IF NOT EXISTS room_appearance_assets_room_idx ON room_appearance_assets(room_id, state)",
    "CREATE INDEX IF NOT EXISTS room_write_budgets_window_idx ON room_write_budgets(window_started_at)",
    "CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_executions_blocking_session_idx ON provider_turn_executions(room_id, session_id) WHERE phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')",
    "CREATE UNIQUE INDEX IF NOT EXISTS provider_turn_executions_blocking_runtime_idx ON provider_turn_executions(runtime_handle_id) WHERE phase IN ('assigned', 'start_dispatching', 'running', 'interrupt_pending', 'quiescing', 'start_ambiguous', 'interrupt_ambiguous', 'recovery_required')",
];

pub(crate) fn statements() -> impl Iterator<Item = &'static str> {
    TABLES
        .iter()
        .map(|table| table.ddl)
        .chain(INDEXES.iter().copied())
}

pub(crate) fn product_tables() -> impl Iterator<Item = &'static TableDefinition> {
    TABLES.iter().filter(|table| !table.infrastructure)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use sqlx::{Row, SqlitePool, sqlite::SqlitePoolOptions};

    use super::{TABLES, product_tables, statements};

    async fn installed_schema() -> SqlitePool {
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
        let declared = TABLES
            .iter()
            .map(|table| table.name.to_owned())
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, declared);

        let product = product_tables()
            .map(|table| table.name)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            product.len(),
            TABLES.iter().filter(|table| !table.infrastructure).count()
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
    async fn room_asset_custody_moves_from_uploader_to_room_in_the_schema() {
        let pool = installed_schema().await;
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES ('user-1', 'participant-1', '{}')",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert profile: {error}"));
        sqlx::query(
            "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, created_by_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_00000000000000000000000000000000', 'general', 'user-1', 'user-1', 'pending.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'pending', 1)",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert pending asset: {error}"));
        sqlx::query(
            "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, created_by_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_11111111111111111111111111111111', 'general', NULL, 'user-1', 'bound.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'bound', NULL)",
        )
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert bound asset: {error}"));

        assert!(
            sqlx::query(
                "INSERT INTO room_appearance_assets(asset_id, room_id, pending_owner_user_id, created_by_user_id, filename, content_type, content, size, created_at, state, expires_at) VALUES ('ra_22222222222222222222222222222222', 'general', NULL, 'user-1', 'invalid.png', 'image/png', X'00', 1, '2026-08-26T00:00:00Z', 'pending', NULL)",
            )
            .execute(&pool)
            .await
            .is_err()
        );

        sqlx::query("DELETE FROM user_profiles WHERE user_id = 'user-1'")
            .execute(&pool)
            .await
            .unwrap_or_else(|error| panic!("delete uploader: {error}"));
        let remaining = sqlx::query_scalar::<_, String>(
            "SELECT asset_id FROM room_appearance_assets ORDER BY asset_id",
        )
        .fetch_all(&pool)
        .await
        .unwrap_or_else(|error| panic!("read remaining assets: {error}"));
        assert_eq!(
            remaining,
            vec!["ra_11111111111111111111111111111111".to_owned()]
        );
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

    async fn seed_human_authorities(pool: &SqlitePool) {
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

    struct SessionAuthority<'a> {
        marker: u8,
        invite_id: &'a str,
        key_kind: &'a str,
        room_id: &'a str,
        user_id: &'a str,
        participant_id: &'a str,
        invite_scope: &'a str,
        reusable_identity: Option<Vec<u8>>,
    }

    async fn insert_human_session(
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
        .bind(vec![session.marker.wrapping_add(0x20); 32])
        .bind(session.reusable_identity)
        .execute(pool)
        .await
        .map(|_| ())
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

        for invalid in [
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
        ] {
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
    async fn invite_limits_preserve_configured_values_and_effective_ceilings() {
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

        for (index, (configured, effective)) in [
            (0_i64, 128_i64),
            (1, 1),
            (2, 2),
            (3, 3),
            (5, 5),
            (128, 128),
            (129, 128),
        ]
        .into_iter()
        .enumerate()
        {
            let accepted_id = format!("{:016x}", index + 1);
            sqlx::query(
                "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'room-a', 'participant-a', 'Guest', 'read_write', ?, ?, 200, 0, 'user-a', 100)",
            )
            .bind(&accepted_id)
            .bind(vec![index as u8 + 1; 32])
            .bind(vec![index as u8 + 0x21; 32])
            .bind(configured)
            .bind(effective)
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

            let rejected_id = format!("{:016x}", index + 0x101);
            assert!(
                sqlx::query(
                    "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'room-a', 'participant-a', 'Guest', 'read_write', ?, ?, 200, 0, 'user-a', 100)",
                )
                .bind(rejected_id)
                .bind(vec![index as u8 + 0x41; 32])
                .bind(vec![index as u8 + 0x61; 32])
                .bind(configured)
                .bind(effective + 1)
                .execute(&pool)
                .await
                .is_err()
            );
        }
    }
}
