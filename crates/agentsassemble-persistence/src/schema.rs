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
        ddl: "CREATE TABLE IF NOT EXISTS runtime_host_identity (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), server_id TEXT NOT NULL UNIQUE, public_key BLOB NOT NULL CHECK(length(public_key) = 32))",
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
        ddl: "CREATE TABLE IF NOT EXISTS user_profiles (user_id TEXT PRIMARY KEY, participant_id TEXT NOT NULL UNIQUE, profile_json TEXT NOT NULL)",
        infrastructure: false,
    },
    TableDefinition {
        name: "profile_attachments",
        ddl: "CREATE TABLE IF NOT EXISTS profile_attachments (attachment_id TEXT PRIMARY KEY, owner_user_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size >= 0 AND size <= 10485760), created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'bound')), expires_at INTEGER, FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
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

    use sqlx::{Row, SqlitePool};

    use super::{TABLES, product_tables, statements};

    #[tokio::test]
    async fn installed_table_set_and_product_inventory_share_one_descriptor_owner() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open in-memory schema database: {error}"));
        for statement in statements() {
            sqlx::query(statement)
                .execute(&pool)
                .await
                .unwrap_or_else(|error| panic!("install schema statement: {error}"));
        }
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
}
