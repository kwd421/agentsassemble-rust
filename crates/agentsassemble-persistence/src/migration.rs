use agentsassemble_domain::DurableAgentSession;
use sqlx::{Row, Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore};

pub(crate) const CURRENT_SCHEMA_VERSION: i64 = 8;

impl SqliteStore {
    pub(crate) async fn migrate_schema(&self) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_optional(&mut *transaction)
            .await?
            .map(|row| row.get::<String, _>("value"));
        let version = match stored {
            Some(value) => value
                .parse::<i64>()
                .ok()
                .filter(|version| *version >= 1)
                .ok_or_else(|| PersistenceError::InvalidSchemaVersion(value.clone()))?,
            None => 1,
        };
        if version > CURRENT_SCHEMA_VERSION {
            return Err(PersistenceError::UnsupportedSchemaVersion {
                found: version,
                supported: CURRENT_SCHEMA_VERSION,
            });
        }
        if version < 2 {
            sqlx::query(
                "CREATE TABLE agent_sessions (room_id TEXT NOT NULL, session_id TEXT NOT NULL, session_json TEXT NOT NULL, PRIMARY KEY(room_id, session_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
            )
            .execute(&mut *transaction)
            .await?;
        }
        if version < 3 {
            redact_legacy_agent_results(&mut transaction).await?;
        }
        if version < 4 {
            reject_unmigratable_lifecycle_intents(&mut transaction).await?;
            sqlx::query(
                "CREATE TABLE lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
            )
            .execute(&mut *transaction)
            .await?;
        }
        if version < 5 {
            sqlx::query(
                "ALTER TABLE lifecycle_command_reservations ADD COLUMN status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'owner_lost'))",
            )
            .execute(&mut *transaction)
            .await?;
        }
        if version < 6 {
            sqlx::query(
                "ALTER TABLE lifecycle_command_reservations ADD COLUMN phase TEXT NOT NULL DEFAULT 'lifecycle_prepared'",
            )
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "ALTER TABLE lifecycle_command_reservations ADD COLUMN prepared_result_json TEXT NOT NULL DEFAULT '{}'",
            )
            .execute(&mut *transaction)
            .await?;
        }
        if version < 7 {
            sqlx::query(
                "CREATE TABLE user_profiles (user_id TEXT PRIMARY KEY, participant_id TEXT NOT NULL UNIQUE, profile_json TEXT NOT NULL)",
            )
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "CREATE TABLE profile_attachments (attachment_id TEXT PRIMARY KEY, owner_user_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp')), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size >= 0 AND size <= 10485760), created_at TEXT NOT NULL, FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
            )
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "CREATE INDEX profile_attachments_owner_idx ON profile_attachments(owner_user_id)",
            )
            .execute(&mut *transaction)
            .await?;
            crate::profile_store::migrate_local_profile_authority(&mut transaction).await?;
        }
        if version < 8 {
            migrate_profile_and_publication_v8(&mut transaction).await?;
        }
        sqlx::query(
            "INSERT INTO runtime_metadata(key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(CURRENT_SCHEMA_VERSION.to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn migrate_profile_and_publication_v8(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    sqlx::query(
        "ALTER TABLE profile_attachments ADD COLUMN state TEXT NOT NULL DEFAULT 'quarantined' CHECK(state IN ('pending', 'bound', 'quarantined'))",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query("ALTER TABLE profile_attachments ADD COLUMN expires_at INTEGER")
        .execute(&mut **transaction)
        .await?;
    crate::profile_attachments::migrate_profile_attachments(transaction).await?;
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS room_event_publication_cursors (room_id TEXT PRIMARY KEY, published_seq INTEGER NOT NULL CHECK(published_seq >= 0), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO room_event_publication_cursors(room_id, published_seq) SELECT rooms.room_id, COALESCE(MAX(room_events.seq), 0) FROM rooms LEFT JOIN room_events ON room_events.room_id = rooms.room_id GROUP BY rooms.room_id",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn reject_unmigratable_lifecycle_intents(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    let sessions = sqlx::query_scalar::<_, String>("SELECT session_json FROM agent_sessions")
        .fetch_all(&mut **transaction)
        .await?;
    for encoded in sessions {
        let session = serde_json::from_str::<DurableAgentSession>(&encoded)?;
        if !session.lifecycle_intent_action.is_empty()
            || !session.lifecycle_intent_id.is_empty()
            || !session.lifecycle_intent_status.is_empty()
        {
            return Err(PersistenceError::IncompleteLifecycleMigration);
        }
    }
    Ok(())
}

async fn redact_legacy_agent_results(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), PersistenceError> {
    let rows =
        sqlx::query("SELECT rowid, result_json FROM command_results WHERE action = 'agent.create'")
            .fetch_all(&mut **transaction)
            .await?;
    for row in rows {
        let rowid = row.get::<i64, _>("rowid");
        let mut result: serde_json::Value =
            serde_json::from_str(row.get::<String, _>("result_json").as_str())?;
        if let Some(session) = result
            .get_mut("agent_session")
            .and_then(serde_json::Value::as_object_mut)
        {
            for private in [
                "workspace",
                "workspace_identity",
                "executable",
                "executable_identity",
                "runtime_profile_key",
            ] {
                session.remove(private);
            }
        }
        sqlx::query("UPDATE command_results SET result_json = ? WHERE rowid = ?")
            .bind(serde_json::to_string(&result)?)
            .bind(rowid)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use agentsassemble_domain::{Participant, ParticipantStatus, Room, RoomSettings, UserProfile};
    use chrono::Utc;
    use image::{DynamicImage, ImageBuffer, ImageFormat, Rgba};
    use sqlx::Row as _;

    use crate::SqliteStore;

    #[tokio::test]
    async fn version_two_command_results_are_redacted_before_replay() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        drop_v7_profile_tables(&store).await;
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert legacy room: {error}"));
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES ('general', 'operator', 'request', 'agent.create', 'hash', ?)",
        )
        .bind(r#"{"agent_session":{"session_id":"agent-1","workspace":"/private","workspace_identity":"workspace-id","executable":"/bin/provider","executable_identity":"executable-id","runtime_profile_key":"profile"},"status":"created"}"#)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert legacy result: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '2' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set legacy version: {error}"));
        sqlx::query("DROP TABLE lifecycle_command_reservations")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v4 table: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate legacy result: {error}"));
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT result_json FROM command_results WHERE request_id = 'request'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated result: {error}"));
        let result: serde_json::Value = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode migrated result: {error}"));
        assert_eq!(result["agent_session"]["session_id"], "agent-1");
        for private in [
            "workspace",
            "workspace_identity",
            "executable",
            "executable_identity",
            "runtime_profile_key",
        ] {
            assert!(result["agent_session"].get(private).is_none());
        }
    }

    #[tokio::test]
    async fn version_one_authority_is_upgraded_before_snapshots_use_sessions() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        drop_v7_profile_tables(&store).await;
        sqlx::query("DROP TABLE agent_sessions")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v2 table: {error}"));
        sqlx::query("DROP TABLE lifecycle_command_reservations")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v4 table: {error}"));
        sqlx::query("DELETE FROM runtime_metadata WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove version marker: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate store: {error}"));
        let table_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'agent_sessions'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect table: {error}"));
        let reservation_table_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'lifecycle_command_reservations'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect reservation table: {error}"));
        let version = sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key = 'schema_version'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("inspect version: {error}"));
        assert_eq!(table_count, 1);
        assert_eq!(reservation_table_count, 1);
        assert_eq!(version, super::CURRENT_SCHEMA_VERSION.to_string());
    }

    #[tokio::test]
    async fn invalid_schema_version_is_not_reinterpreted_as_a_migratable_authority() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = 'corrupt' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("corrupt version marker: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(crate::PersistenceError::InvalidSchemaVersion(value)) if value == "corrupt"
        ));
    }

    #[tokio::test]
    async fn version_four_reservations_gain_pending_status() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        drop_v7_profile_tables(&store).await;
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
        sqlx::query("DROP TABLE lifecycle_command_reservations")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v5 table: {error}"));
        sqlx::query(
            "CREATE TABLE lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("create v4 table: {error}"));
        sqlx::query(
            "INSERT INTO lifecycle_command_reservations(room_id, principal_id, request_id, action, payload_hash, session_id, operation_id) VALUES ('general', 'operator', 'pending-stop', 'agent.stop', 'hash', 'agent-1', 'operation-1')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert v4 reservation: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '4' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set v4 marker: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate v4 store: {error}"));
        let status = sqlx::query_scalar::<_, String>(
            "SELECT status FROM lifecycle_command_reservations WHERE request_id = 'pending-stop'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated reservation: {error}"));
        assert_eq!(status, "pending");
    }

    #[tokio::test]
    async fn version_five_reservations_gain_explicit_phase_and_result() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        drop_v7_profile_tables(&store).await;
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
        sqlx::query("DROP TABLE lifecycle_command_reservations")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v6 table: {error}"));
        sqlx::query(
            "CREATE TABLE lifecycle_command_reservations (room_id TEXT NOT NULL, principal_id TEXT NOT NULL, request_id TEXT NOT NULL, action TEXT NOT NULL, payload_hash TEXT NOT NULL, session_id TEXT NOT NULL, operation_id TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'pending' CHECK(status IN ('pending', 'owner_lost')), PRIMARY KEY(room_id, principal_id, request_id), FOREIGN KEY(room_id) REFERENCES rooms(room_id) ON DELETE CASCADE)",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("create v5 table: {error}"));
        sqlx::query(
            "INSERT INTO lifecycle_command_reservations(room_id, principal_id, request_id, action, payload_hash, session_id, operation_id) VALUES ('general', 'operator', 'pending-start', 'agent.start', 'hash', 'agent-1', 'operation-1')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert v5 reservation: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '5' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set v5 marker: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate v5 store: {error}"));
        let row = sqlx::query(
            "SELECT phase, prepared_result_json FROM lifecycle_command_reservations WHERE request_id = 'pending-start'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated reservation: {error}"));
        assert_eq!(row.get::<String, _>("phase"), "lifecycle_prepared");
        assert_eq!(row.get::<String, _>("prepared_result_json"), "{}");
    }

    #[tokio::test]
    async fn version_three_incomplete_lifecycle_authority_fails_closed() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        sqlx::query(
            "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
        let session = serde_json::json!({
            "room_id": "general",
            "session_id": "agent-1",
            "participant_id": "agent-1",
            "display_name": "Agent",
            "status": "available",
            "runtime_status": "starting",
            "enabled": true,
            "provider_kind": "codex_live_session",
            "runtime_kind": "live_cli",
            "connection_kind": "native_cli_bridge",
            "external_owned": false,
            "process_ownership": "server",
            "model": "gpt-5.6-terra",
            "reasoning_effort": "medium",
            "service_tier": "default",
            "variant": "",
            "execution_harness": "builtin",
            "permission_mode": "meeting_read_only",
            "max_output_tokens": 0,
            "catalog_revision": "catalog-1",
            "transport": "stdio_jsonl",
            "last_seen_event_id": "",
            "last_seen_seq": 0,
            "last_provider_sync_event_id": "",
            "last_provider_sync_seq": 0,
            "bootstrap_cutoff_seq": 0,
            "turn_count": 0,
            "created_at": "2026-01-01T00:00:00Z",
            "updated_at": "2026-01-01T00:00:00Z",
            "workspace": "/workspace",
            "runtime_profile_key": "profile-1",
            "lifecycle_intent_action": "start",
            "lifecycle_intent_id": "unrecoverable-request-binding",
            "lifecycle_intent_status": "prepared"
        });
        sqlx::query(
            "INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES ('general', 'agent-1', ?)",
        )
        .bind(session.to_string())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert incomplete session: {error}"));
        sqlx::query("DROP TABLE lifecycle_command_reservations")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v4 table: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '3' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set v3 marker: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(crate::PersistenceError::IncompleteLifecycleMigration)
        ));
    }

    #[tokio::test]
    async fn version_six_rooms_adopt_the_new_local_profile_authority() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "Host".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: "room-owned".to_owned(),
            muted: true,
            created_at: now,
            updated_at: now,
        };
        store
            .initialize_room(
                &room,
                &RoomSettings::defaults("General".to_owned()),
                &participant,
            )
            .await
            .unwrap_or_else(|error| panic!("initialize v6 room: {error}"));
        drop_v7_profile_tables(&store).await;
        sqlx::query("UPDATE runtime_metadata SET value = '6' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set v6 marker: {error}"));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate v6 store: {error}"));
        let profile: UserProfile = serde_json::from_str(
            &sqlx::query_scalar::<_, String>(
                "SELECT profile_json FROM user_profiles WHERE user_id = 'operator-local-user'",
            )
            .fetch_one(&reopened.pool)
            .await
            .unwrap_or_else(|error| panic!("read migrated profile: {error}")),
        )
        .unwrap_or_else(|error| panic!("decode migrated profile: {error}"));
        let membership = reopened
            .participant("general", "operator-local")
            .await
            .unwrap_or_else(|error| panic!("read migrated participant: {error}"));
        assert_eq!(profile.display_name, "SeiNel");
        assert_eq!(membership.display_name, profile.display_name);
        assert_eq!(membership.role, "host");
        assert_eq!(membership.owner_id, "room-owned");
        assert!(membership.muted);
        assert_eq!(membership.status, ParticipantStatus::Joined);
        let event_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
            .fetch_one(&reopened.pool)
            .await
            .unwrap_or_else(|error| panic!("count migration events: {error}"));
        assert_eq!(event_count, 0);
    }

    #[tokio::test]
    async fn version_seven_binds_only_referenced_validated_avatars_and_seeds_publication() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create store: {error}"));
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "SeiNel".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        store
            .initialize_room(
                &room,
                &RoomSettings::defaults("General".to_owned()),
                &participant,
            )
            .await
            .unwrap_or_else(|error| panic!("initialize v7 room: {error}"));
        install_v7_avatar_fixture(&store, now).await;
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("migrate v7 store: {error}"));
        reopened
            .profile_attachment("legacyavatar01")
            .await
            .unwrap_or_else(|error| panic!("read migrated bound avatar: {error}"));
        let states = sqlx::query_as::<_, (String, String)>(
            "SELECT attachment_id, state FROM profile_attachments ORDER BY attachment_id",
        )
        .fetch_all(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated attachment states: {error}"));
        assert_eq!(
            states,
            vec![
                ("legacyavatar01".to_owned(), "bound".to_owned()),
                ("legacyorphan01".to_owned(), "quarantined".to_owned())
            ]
        );
        let cursor = sqlx::query_scalar::<_, i64>(
            "SELECT published_seq FROM room_event_publication_cursors WHERE room_id = 'general'",
        )
        .fetch_one(&reopened.pool)
        .await
        .unwrap_or_else(|error| panic!("read migrated publication cursor: {error}"));
        assert_eq!(cursor, 0);
    }

    async fn install_v7_avatar_fixture(store: &SqliteStore, now: chrono::DateTime<Utc>) {
        sqlx::query("DROP TABLE profile_attachments")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("drop current attachment table: {error}"));
        sqlx::query(
            "CREATE TABLE profile_attachments (attachment_id TEXT PRIMARY KEY, owner_user_id TEXT NOT NULL, filename TEXT NOT NULL, content_type TEXT NOT NULL CHECK(content_type IN ('image/png', 'image/jpeg', 'image/gif', 'image/webp')), content BLOB NOT NULL, size INTEGER NOT NULL CHECK(size >= 0 AND size <= 10485760), created_at TEXT NOT NULL, FOREIGN KEY(owner_user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE)",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("create v7 attachment table: {error}"));
        let png = valid_png();
        for (attachment_id, content) in [
            ("legacyavatar01", png.clone()),
            ("legacyorphan01", b"untrusted orphan".to_vec()),
        ] {
            sqlx::query(
                "INSERT INTO profile_attachments(attachment_id, owner_user_id, filename, content_type, content, size, created_at) VALUES (?, 'operator-local-user', 'legacy.png', 'image/png', ?, ?, ?)",
            )
            .bind(attachment_id)
            .bind(&content)
            .bind(i64::try_from(content.len()).unwrap_or_default())
            .bind(now.to_rfc3339())
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("insert v7 attachment: {error}"));
        }
        let mut profile: UserProfile = serde_json::from_str(
            &sqlx::query_scalar::<_, String>(
                "SELECT profile_json FROM user_profiles WHERE user_id = 'operator-local-user'",
            )
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read v7 profile: {error}")),
        )
        .unwrap_or_else(|error| panic!("decode v7 profile: {error}"));
        profile.avatar_image_url = "/api/attachments/legacyavatar01?view=1".to_owned();
        sqlx::query(
            "UPDATE user_profiles SET profile_json = ? WHERE user_id = 'operator-local-user'",
        )
        .bind(
            serde_json::to_string(&profile)
                .unwrap_or_else(|error| panic!("encode v7 profile: {error}")),
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("bind legacy profile avatar: {error}"));
        sqlx::query("DROP TABLE room_event_publication_cursors")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("drop current publication table: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = '7' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set v7 marker: {error}"));
    }

    fn valid_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(ImageBuffer::from_pixel(2, 2, Rgba([1, 2, 3, 255])));
        let mut encoded = Cursor::new(Vec::new());
        image
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap_or_else(|error| panic!("encode v7 png: {error}"));
        encoded.into_inner()
    }

    async fn drop_v7_profile_tables(store: &SqliteStore) {
        sqlx::query("DROP TABLE profile_attachments")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v7 attachments table: {error}"));
        sqlx::query("DROP TABLE user_profiles")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("remove v7 profile table: {error}"));
    }
}
