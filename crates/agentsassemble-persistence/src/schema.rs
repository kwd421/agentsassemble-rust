pub(crate) const PRODUCT_TABLES: &[(&str, &str)] = &[
    ("rooms", "SELECT EXISTS(SELECT 1 FROM rooms LIMIT 1)"),
    (
        "room_create_results",
        "SELECT EXISTS(SELECT 1 FROM room_create_results LIMIT 1)",
    ),
    (
        "participants",
        "SELECT EXISTS(SELECT 1 FROM participants LIMIT 1)",
    ),
    (
        "user_profiles",
        "SELECT EXISTS(SELECT 1 FROM user_profiles LIMIT 1)",
    ),
    (
        "profile_attachments",
        "SELECT EXISTS(SELECT 1 FROM profile_attachments LIMIT 1)",
    ),
    (
        "agent_sessions",
        "SELECT EXISTS(SELECT 1 FROM agent_sessions LIMIT 1)",
    ),
    (
        "lifecycle_command_reservations",
        "SELECT EXISTS(SELECT 1 FROM lifecycle_command_reservations LIMIT 1)",
    ),
    (
        "room_events",
        "SELECT EXISTS(SELECT 1 FROM room_events LIMIT 1)",
    ),
    (
        "room_turn_tool_results",
        "SELECT EXISTS(SELECT 1 FROM room_turn_tool_results LIMIT 1)",
    ),
    (
        "room_write_budgets",
        "SELECT EXISTS(SELECT 1 FROM room_write_budgets LIMIT 1)",
    ),
    (
        "room_event_publication_cursors",
        "SELECT EXISTS(SELECT 1 FROM room_event_publication_cursors LIMIT 1)",
    ),
    (
        "command_results",
        "SELECT EXISTS(SELECT 1 FROM command_results LIMIT 1)",
    ),
];

pub(crate) fn statements() -> &'static [&'static str] {
    &[
        "CREATE TABLE IF NOT EXISTS runtime_metadata (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS local_bootstrap_authority (singleton INTEGER PRIMARY KEY CHECK(singleton = 1), authority_lineage_id TEXT NOT NULL UNIQUE, state TEXT NOT NULL CHECK(state IN ('empty', 'initializing', 'complete')), request_id TEXT NOT NULL DEFAULT '', schema_revision INTEGER NOT NULL CHECK(schema_revision > 0), initialization_digest TEXT NOT NULL DEFAULT '', user_id TEXT NOT NULL DEFAULT '', participant_id TEXT NOT NULL DEFAULT '', result_json TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL, completed_at TEXT)",
        "CREATE TABLE IF NOT EXISTS rooms (room_id TEXT PRIMARY KEY, room_json TEXT NOT NULL, settings_json TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS room_create_results (principal_id TEXT NOT NULL, request_id TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(principal_id, request_id))",
        "CREATE TABLE IF NOT EXISTS participants (room_id TEXT NOT NULL, participant_id TEXT NOT NULL, participant_json TEXT NOT NULL, PRIMARY KEY(room_id, participant_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS user_profiles (user_id TEXT PRIMARY KEY, participant_id TEXT NOT NULL UNIQUE, profile_json TEXT NOT NULL)",
        "CREATE TABLE IF NOT EXISTS profile_attachments (attachment_id TEXT PRIMARY KEY, owner_user_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type = 'image/png'), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size >= 0 AND size <= 10485760), created_at TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending', 'bound')), expires_at INTEGER, FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        "CREATE INDEX IF NOT EXISTS profile_attachments_owner_idx ON profile_attachments(owner_user_id)",
        "CREATE TABLE IF NOT EXISTS agent_sessions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, session_json TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'owner_lost')), phase TEXT NOT NULL DEFAULT 'lifecycle_prepared', prepared_result_json TEXT NOT NULL DEFAULT '{}', PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS room_events (room_id TEXT NOT NULL, seq INTEGER NOT NULL CHECK(seq > 0), event_json TEXT NOT NULL, PRIMARY KEY(room_id, seq), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS room_turn_tool_results (room_id TEXT NOT NULL, session_id TEXT NOT NULL, turn_id TEXT NOT NULL, result_id TEXT NOT NULL, event_seq INTEGER NOT NULL CHECK(event_seq > 0), PRIMARY KEY(room_id, result_id), FOREIGN KEY(room_id, session_id) REFERENCES agent_sessions(room_id, session_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS room_write_budgets (room_id TEXT NOT NULL, window_started_at INTEGER NOT NULL, command_count INTEGER NOT NULL CHECK(command_count >= 0), payload_bytes INTEGER NOT NULL CHECK(payload_bytes >= 0), PRIMARY KEY(room_id, window_started_at), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE INDEX IF NOT EXISTS room_write_budgets_window_idx ON room_write_budgets(window_started_at)",
        "CREATE TABLE IF NOT EXISTS room_event_publication_cursors (room_id TEXT PRIMARY KEY, published_seq INTEGER NOT NULL CHECK(published_seq >= 0), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        "CREATE TABLE IF NOT EXISTS command_results (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, result_json TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
    ]
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{PRODUCT_TABLES, statements};

    #[test]
    fn product_table_inventory_matches_every_non_infrastructure_table() {
        let declared = statements()
            .iter()
            .filter_map(|statement| {
                statement
                    .strip_prefix("CREATE TABLE IF NOT EXISTS ")
                    .and_then(|suffix| suffix.split_once(' '))
                    .map(|(table, _)| table)
            })
            .filter(|table| !matches!(*table, "runtime_metadata" | "local_bootstrap_authority"))
            .collect::<BTreeSet<_>>();
        let inventoried = PRODUCT_TABLES
            .iter()
            .map(|(name, _)| *name)
            .collect::<BTreeSet<_>>();

        assert_eq!(inventoried, declared);
        assert_eq!(inventoried.len(), PRODUCT_TABLES.len());
        for (name, has_rows_sql) in PRODUCT_TABLES {
            let query_table = has_rows_sql
                .strip_prefix("SELECT EXISTS(SELECT 1 FROM ")
                .and_then(|suffix| suffix.strip_suffix(" LIMIT 1)"));
            assert_eq!(query_table, Some(*name));
        }
    }
}
