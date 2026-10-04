use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::PersistenceError;

pub const CURRENT_SCHEMA_VERSION: i64 = 80;

// Historical metadata remains only to preserve v74 rows and their foreign keys.
// It is never promoted or used as current host admission authority.
pub(crate) const V74_OWNER_SESSIONS_DDL: &str = "CREATE TABLE central_owner_sessions (
    fingerprint BLOB PRIMARY KEY CHECK(length(fingerprint)=32),
    connection_id TEXT NOT NULL UNIQUE, server_id TEXT NOT NULL,
    person_id TEXT NOT NULL, device_id TEXT NOT NULL,
    browser_fingerprint BLOB NOT NULL CHECK(length(browser_fingerprint)=32),
    origin TEXT NOT NULL, generation INTEGER NOT NULL,
    session_expires_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
    renew_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))) STRICT";

pub(crate) async fn validate_schema_version(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
        .fetch_optional(pool)
        .await?
        .map(|row| row.get::<String, _>("value"))
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion("missing".to_owned()))?;
    let found = stored
        .parse::<i64>()
        .ok()
        .filter(|version| *version >= 1)
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion(stored.clone()))?;
    if !(70..=CURRENT_SCHEMA_VERSION).contains(&found) {
        return Err(PersistenceError::SchemaVersionMismatch {
            found,
            required: CURRENT_SCHEMA_VERSION,
        });
    }
    let server_id = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'server_id'",
    )
    .fetch_optional(pool)
    .await?
    .ok_or(PersistenceError::InvalidServerId)?;
    uuid::Uuid::parse_str(&server_id)
        .map(|_| ())
        .map_err(|_| PersistenceError::InvalidServerId)
}

// Called only after the existing host key and database authority have been verified.
// Upgrades retain product rows but retire legacy remote authority without provable custody.
pub(crate) async fn upgrade_schema(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version == "70" {
        sqlx::query(crate::message_attachments::connector::DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '71' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if version == "70" || version == "71" {
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN central_owner INTEGER NOT NULL DEFAULT 0 CHECK(central_owner IN (0, 1))")
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '72' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if matches!(version.as_str(), "70" | "71" | "72") {
        sqlx::query(crate::server_owner_authority::GRANT_DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '73' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if matches!(version.as_str(), "70" | "71" | "72" | "73") {
        sqlx::query(V74_OWNER_SESSIONS_DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN owner_session_fingerprint BLOB REFERENCES central_owner_sessions(fingerprint) ON DELETE CASCADE").execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '74' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if matches!(version.as_str(), "70" | "71" | "72" | "73" | "74") {
        sqlx::query(crate::host_owner_session::DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN host_owner_session_fingerprint BLOB REFERENCES host_owner_sessions(fingerprint) ON DELETE CASCADE").execute(&mut *tx).await?;
        sqlx::query(
            "ALTER TABLE operator_pairings ADD COLUMN device_name TEXT NOT NULL DEFAULT ''",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN browser TEXT NOT NULL DEFAULT ''")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN os TEXT NOT NULL DEFAULT ''")
            .execute(&mut *tx)
            .await?;
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN last_connected_at INTEGER")
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "UPDATE operator_pairings SET revoked = 1 WHERE owner_session_fingerprint IS NOT NULL",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE central_owner_sessions SET revoked = 1")
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '75' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if version.parse::<i64>().is_ok_and(|version| version < 76) {
        // v72/v73 central room sessions never had historical parent custody.
        // Also repair databases already opened by v75; native pairings stay valid.
        sqlx::query("UPDATE operator_pairings SET revoked = 1 WHERE central_owner = 1 AND host_owner_session_fingerprint IS NULL")
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '76' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if version.parse::<i64>().is_ok_and(|version| version < 77) {
        // Old unmarked pairings cannot prove native versus central issuance.
        // Retire used sessions and unused grants once, including v76 survivors;
        // attendee authority is denied by its existing parent revalidation.
        sqlx::query(
            "UPDATE operator_pairings SET revoked = 1 WHERE host_owner_session_fingerprint IS NULL",
        )
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '77' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    upgrade_turn_custody(&mut tx, &version).await?;
    if version.parse::<i64>().is_ok_and(|version| version < 80) {
        // Preserve expired/revoked rows. Only still-live native custody gains idle expiry.
        sqlx::query("UPDATE operator_pairings SET last_connected_at = COALESCE(last_connected_at, (session_expires_at / 1000000) - 3600), session_expires_at = 0 WHERE central_owner = 0 AND host_owner_session_fingerprint IS NULL AND revoked = 0 AND session_expires_at > ?")
            .bind(chrono::Utc::now().timestamp_micros()).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '80' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

// Turn custody upgrades share the receipt and immutable-input routing boundary.
async fn upgrade_turn_custody(
    tx: &mut Transaction<'_, Sqlite>,
    version: &str,
) -> Result<(), PersistenceError> {
    if version.parse::<i64>().is_ok_and(|version| version < 78) {
        sqlx::query("ALTER TABLE provider_turn_executions ADD COLUMN released_input_ids TEXT CHECK(released_input_ids IS NULL OR (json_valid(released_input_ids) AND json_type(released_input_ids) = 'array'))")
            .execute(&mut **tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '78' WHERE key = 'schema_version'")
            .execute(&mut **tx)
            .await?;
    }
    if version.parse::<i64>().is_ok_and(|version| version < 79) {
        sqlx::query(crate::ordered_input_route::DDL)
            .execute(&mut **tx)
            .await?;
        crate::ordered_input_route::upgrade_retained_inputs(tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '79' WHERE key = 'schema_version'")
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CURRENT_SCHEMA_VERSION;
    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn supported_upgrade_preserves_bootstrap_room_and_local_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        for previous in [70, 71, 72, 73, 74] {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("runtime.sqlite3");
            let store = SqliteStore::open_path(&path).await?;
            sqlx::query("UPDATE local_bootstrap_authority SET schema_revision = 70")
                .execute(&store.pool)
                .await?;
            let request_id = uuid::Uuid::new_v4().to_string();
            store
                .bootstrap_local_authority(&request_id, "Preserved host")
                .await?;
            store
                .create_room_for_local_operator(
                    &uuid::Uuid::new_v4().to_string(),
                    "preserved",
                    "Preserved room",
                )
                .await?;
            let before: String =
                sqlx::query_scalar("SELECT room_json FROM rooms WHERE room_id = 'preserved'")
                    .fetch_one(&store.pool)
                    .await?;
            let manager = store
                .authorize_local_room_manager(
                    "preserved",
                    agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                    agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await?;
            store
                .create_operator_pairing(
                    &crate::RoomManagerAuthority::Local(manager.clone()),
                    &[31; 32],
                    "https://preserved.example.test",
                    chrono::Utc::now(),
                )
                .await?;
            simulate_previous_schema(&store, previous).await?;
            sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
                .bind(previous.to_string())
                .execute(&store.pool)
                .await?;
            drop(store);
            let reopened = SqliteStore::open_path(&path).await?;
            assert!(
                reopened
                    .bootstrap_local_authority(&request_id, "Preserved host")
                    .await?
                    .deduplicated
            );
            assert_eq!(
                before,
                sqlx::query_scalar::<_, String>(
                    "SELECT room_json FROM rooms WHERE room_id = 'preserved'"
                )
                .fetch_one(&reopened.pool)
                .await?
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_connector_uploads")
                    .fetch_one(&reopened.pool)
                    .await?,
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT central_owner FROM operator_pairings")
                    .fetch_one(&reopened.pool)
                    .await?,
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, String>(
                    "SELECT value FROM runtime_metadata WHERE key = 'schema_version'"
                )
                .fetch_one(&reopened.pool)
                .await?,
                CURRENT_SCHEMA_VERSION.to_string()
            );
            drop(reopened);
            SqliteStore::open_path(&path).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn v72_parentless_remote_authority_is_retired() -> TestResult {
        parentless_remote_authority_is_retired(72).await
    }

    #[tokio::test]
    async fn v73_parentless_remote_authority_is_retired() -> TestResult {
        parentless_remote_authority_is_retired(73).await
    }

    #[tokio::test]
    async fn v75_parentless_remote_authority_is_retired() -> TestResult {
        parentless_remote_authority_is_retired(75).await
    }

    #[tokio::test]
    async fn v76_parentless_remote_authority_is_retired() -> TestResult {
        parentless_remote_authority_is_retired(76).await
    }

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    struct LegacyRemoteAuthority {
        root: crate::OperatorSessionAuthorization,
        child: crate::OperatorSessionAuthorization,
        invites: Vec<[u8; 32]>,
        attendee: crate::AttendeeAdmission,
        join_id: uuid::Uuid,
    }

    fn attendee_admission(
        fingerprint: &[u8; 32],
        request_id: uuid::Uuid,
    ) -> crate::AttendeeAdmissionRequest<'_> {
        crate::AttendeeAdmissionRequest {
            invite_fingerprint: fingerprint,
            client_fingerprint: &[9; 32],
            request_id,
            provider_kind: "codex_live_session",
            display_name: "Companion",
        }
    }

    async fn legacy_remote_authority(
        store: &SqliteStore,
    ) -> Result<LegacyRemoteAuthority, Box<dyn std::error::Error>> {
        use crate::{RoomManagerAuthority, RoomSessionAuthorization};
        use chrono::Utc;
        use sha2::{Digest, Sha256};
        store
            .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Host")
            .await?;
        store
            .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
            .await?;
        let manager = store
            .authorize_local_room_manager(
                "room",
                agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?;
        let origin = "https://owner.example.test";
        store
            .create_operator_pairing(
                &RoomManagerAuthority::Local(manager),
                &[31; 32],
                origin,
                Utc::now(),
            )
            .await?;
        let root = store
            .redeem_operator_pairing(&[31; 32], &[42; 32], origin, Utc::now())
            .await?
            .authorization;
        // Reproduce the old central owner issuing an unmarked pairing and grant.
        sqlx::query("UPDATE operator_pairings SET central_owner = 1, session_expires_at = ?")
            .bind((Utc::now() + chrono::Duration::hours(1)).timestamp_micros())
            .execute(&store.pool)
            .await?;
        let root = store
            .authorize_operator_session(root.session_fingerprint(), &[42; 32], origin)
            .await?;
        for token in [32, 33] {
            store
                .create_operator_pairing(
                    &RoomManagerAuthority::Operator(Box::new(root.clone())),
                    &[token; 32],
                    origin,
                    Utc::now(),
                )
                .await?;
        }
        let child = store
            .redeem_operator_pairing(&[32; 32], &[43; 32], origin, Utc::now())
            .await?
            .authorization;
        // A used unmarked pairing can perpetuate manager authority before the fix.
        store
            .create_operator_pairing(
                &RoomManagerAuthority::Operator(Box::new(child.clone())),
                &[34; 32],
                origin,
                Utc::now(),
            )
            .await?;
        let issuer = RoomSessionAuthorization::Operator(child.clone());
        let mut invites = Vec::new();
        for _ in 0..2 {
            let invite = store
                .create_companion_attendee_invite(
                    &issuer,
                    crate::CompanionInviteRequest {
                        request_id: uuid::Uuid::new_v4(),
                        provider_kind: "codex_live_session",
                        display_name: "Companion",
                    },
                    Utc::now(),
                )
                .await?;
            invites.push(<[u8; 32]>::from(Sha256::digest(
                invite.invite_bearer.as_bytes(),
            )));
        }
        let join_id = uuid::Uuid::new_v4();
        let attendee = store
            .admit_attendee(attendee_admission(&invites[0], join_id), Utc::now())
            .await?;
        store
            .revalidate_attendee_session(&attendee.authorization, Utc::now())
            .await?;
        Ok(LegacyRemoteAuthority {
            root,
            child,
            invites,
            attendee,
            join_id,
        })
    }

    async fn parentless_remote_authority_is_retired(previous: i32) -> TestResult {
        use crate::RoomManagerAuthority;
        use chrono::Utc;

        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path).await?;
        let LegacyRemoteAuthority {
            root,
            child,
            invites,
            attendee,
            join_id,
        } = legacy_remote_authority(&store).await?;
        let origin = "https://owner.example.test";
        if previous < 75 {
            simulate_previous_schema(&store, previous).await?;
        } else {
            sqlx::query("ALTER TABLE provider_turn_executions DROP COLUMN released_input_ids")
                .execute(&store.pool)
                .await?;
        }
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind(previous.to_string())
            .execute(&store.pool)
            .await?;
        drop(store);
        let reopened = SqliteStore::open_path(&path).await?;
        assert!(
            reopened
                .authorize_operator_session(child.session_fingerprint(), &[43; 32], origin)
                .await
                .is_err(),
            "v{previous} unmarked session survived"
        );
        assert!(
            reopened
                .authorize_operator_session(root.session_fingerprint(), &[42; 32], origin)
                .await
                .is_err()
        );
        assert!(
            reopened
                .create_operator_pairing(
                    &RoomManagerAuthority::Operator(Box::new(child)),
                    &[35; 32],
                    origin,
                    Utc::now()
                )
                .await
                .is_err()
        );
        for token in [31, 32, 33, 34] {
            let device = if token == 31 { 42 } else { 43 };
            assert!(
                reopened
                    .redeem_operator_pairing(&[token; 32], &[device; 32], origin, Utc::now())
                    .await
                    .is_err()
            );
        }
        assert!(
            reopened
                .revalidate_attendee_session(&attendee.authorization, Utc::now())
                .await
                .is_err()
        );
        for invite in &invites {
            assert!(
                reopened
                    .admit_attendee(attendee_admission(invite, join_id), Utc::now())
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM operator_pairings WHERE revoked = 1"
            )
            .fetch_one(&reopened.pool)
            .await?,
            4
        );
        let fresh = reconnect_native_remote(&reopened).await?;
        drop(reopened);
        let reopened = SqliteStore::open_path(&path).await?;
        reopened
            .authorize_operator_session(fresh.session_fingerprint(), &[44; 32], origin)
            .await?;
        Ok(())
    }

    async fn reconnect_native_remote(
        store: &SqliteStore,
    ) -> Result<crate::OperatorSessionAuthorization, Box<dyn std::error::Error>> {
        use crate::RoomManagerAuthority;
        use chrono::Utc;
        let origin = "https://owner.example.test";
        store
            .validate_server_owner(&crate::ServerOwnerAuthority::LocalOperator)
            .await?;
        let manager = store
            .authorize_local_room_manager(
                "room",
                agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?;
        store
            .create_operator_pairing(
                &RoomManagerAuthority::Local(manager),
                &[36; 32],
                origin,
                Utc::now(),
            )
            .await?;
        let fresh = store
            .redeem_operator_pairing(&[36; 32], &[44; 32], origin, Utc::now())
            .await?
            .authorization;
        Ok(fresh)
    }

    async fn simulate_previous_schema(
        store: &SqliteStore,
        previous: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("ALTER TABLE provider_turn_executions DROP COLUMN released_input_ids")
            .execute(&store.pool)
            .await?;
        if previous == 70 {
            sqlx::query("DROP TABLE room_connector_uploads")
                .execute(&store.pool)
                .await?;
        }
        if previous < 72 {
            sqlx::query("ALTER TABLE operator_pairings DROP COLUMN central_owner")
                .execute(&store.pool)
                .await?;
        }
        for statement in [
            "ALTER TABLE operator_pairings DROP COLUMN host_owner_session_fingerprint",
            "ALTER TABLE operator_pairings DROP COLUMN device_name",
            "ALTER TABLE operator_pairings DROP COLUMN browser",
            "ALTER TABLE operator_pairings DROP COLUMN os",
            "ALTER TABLE operator_pairings DROP COLUMN last_connected_at",
        ] {
            sqlx::query(statement).execute(&store.pool).await?;
        }
        sqlx::query("DROP TABLE host_owner_sessions")
            .execute(&store.pool)
            .await?;
        if previous < 74 {
            sqlx::query("ALTER TABLE operator_pairings DROP COLUMN owner_session_fingerprint")
                .execute(&store.pool)
                .await?;
            sqlx::query("DROP TABLE central_owner_sessions")
                .execute(&store.pool)
                .await?;
        }
        if previous < 73 {
            sqlx::query("DROP TABLE central_owner_grants")
                .execute(&store.pool)
                .await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn v74_custody_is_retained_as_history_and_cannot_become_host_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path).await?;
        store
            .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Preserved host")
            .await?;
        store
            .create_room_for_local_operator(
                &uuid::Uuid::new_v4().to_string(),
                "preserved",
                "Preserved room",
            )
            .await?;
        let manager = store
            .authorize_local_room_manager(
                "preserved",
                agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?;
        store
            .create_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager),
                &[1; 32],
                "https://owner.example.test",
                chrono::Utc::now(),
            )
            .await?;
        let before: String =
            sqlx::query_scalar("SELECT room_json FROM rooms WHERE room_id = 'preserved'")
                .fetch_one(&store.pool)
                .await?;
        simulate_previous_schema(&store, 74).await?;
        let server_id = store.local_bootstrap_status().await?.server_id;
        let now = chrono::Utc::now().timestamp();
        sqlx::query("INSERT INTO central_owner_sessions (fingerprint, connection_id, server_id, person_id, device_id, browser_fingerprint, origin, generation, session_expires_at, expires_at, renew_at) VALUES (?, ?, ?, 'person', 'device', ?, 'https://owner.example.test', 1, ?, ?, ?)")
            .bind([33_u8; 32].as_slice()).bind(format!("soc_{}", "a".repeat(43))).bind(server_id)
            .bind([42_u8; 32].as_slice()).bind(now + 3600).bind(now + 60).bind(now + 20)
            .execute(&store.pool).await?;
        sqlx::query(
            "UPDATE operator_pairings SET owner_session_fingerprint = ?, central_owner = 1",
        )
        .bind([33_u8; 32].as_slice())
        .execute(&store.pool)
        .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '74' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await?;
        drop(store);
        let reopened = SqliteStore::open_path(&path).await?;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT revoked FROM central_owner_sessions")
                .fetch_one(&reopened.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT revoked FROM operator_pairings")
                .fetch_one(&reopened.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM host_owner_sessions")
                .fetch_one(&reopened.pool)
                .await?,
            0
        );
        assert!(
            reopened
                .authorize_owner_session(&[33; 32], &[42; 32], "https://owner.example.test")
                .await
                .is_err()
        );
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT room_json FROM rooms WHERE room_id = 'preserved'"
            )
            .fetch_one(&reopened.pool)
            .await?,
            before
        );
        Ok(())
    }

    #[tokio::test]
    async fn older_schema_is_rejected_without_conversion() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create current store: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind("69")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set older schema: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(PersistenceError::SchemaVersionMismatch { found, required })
                if found == 69 && required == CURRENT_SCHEMA_VERSION
        ));
    }
}
