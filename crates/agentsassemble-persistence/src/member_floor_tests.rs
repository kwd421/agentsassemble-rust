//! The contracted minimum v81 fixture, not the future complete C4b migration.
use chrono::Utc;
use sha2::{Digest, Sha256};
use sqlx::Row;

use crate::{
    AccountAuthority, AccountIdentity, GuestRecoveryRequest, HumanSessionAuthorization,
    PersistenceError, SqliteStore,
    human_admission_store::tests::{admitted, fixture, insert_invite, prepared},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn member_ddl() -> String {
    use crate::central_identity_bindings::{TABLE_DDL, USER_INDEX_DDL};
    format!("{TABLE_DDL};\n{USER_INDEX_DDL};\n")
}

async fn install_v81(store: &SqliteStore) -> Result<(), sqlx::Error> {
    let mut tx = store.pool.begin().await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(member_ddl()))
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE runtime_metadata SET value = '81' WHERE key = 'schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}

async fn bind(store: &SqliteStore, user_id: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO central_identity_bindings VALUES ('binding', 'https://identity.example.test', 'person', ?, 100)")
        .bind(user_id).execute(&store.pool).await?;
    Ok(())
}

fn fingerprint(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

async fn guest(
    store: &SqliteStore,
) -> Result<(HumanSessionAuthorization, AccountIdentity), PersistenceError> {
    let now = Utc::now();
    insert_invite(store, [1; 32], [2; 32], "guest", 10, now).await;
    let commit = admitted(
        store
            .admit_human(
                &prepared(
                    [2; 32],
                    [3; 32],
                    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    "Guest",
                ),
                now,
            )
            .await?,
    );
    let session = store
        .authorize_human_session(&fingerprint(commit.session_bearer()))
        .await?;
    let identity = store
        .account_identity(AccountAuthority::HumanSession {
            authorization: session.clone(),
            browser_fingerprint: [3; 32],
        })
        .await?;
    Ok((session, identity))
}

fn unsupported<T>(result: Result<T, PersistenceError>) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. })
            if code == "central_member_unsupported" => {}
        _ => panic!("expected unsupported member rejection"),
    }
}

// Snapshot every table row and schema object, not just the version marker.
async fn snapshot(store: &SqliteStore) -> Result<Vec<String>, sqlx::Error> {
    let mut result = sqlx::query_scalar::<_, String>(
        "SELECT type || ':' || name || ':' || COALESCE(sql, '') FROM sqlite_master ORDER BY type, name")
        .fetch_all(&store.pool).await?;
    let tables = sqlx::query_scalar::<_, String>(
        "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
    )
    .fetch_all(&store.pool)
    .await?;
    for table in tables {
        result.push(table.clone());
        let quoted = format!("\"{}\"", table.replace('"', "\"\""));
        let columns = sqlx::query("SELECT name FROM pragma_table_info(?)")
            .bind(&table)
            .fetch_all(&store.pool)
            .await?;
        let expressions = columns
            .iter()
            .map(|row| {
                format!(
                    "quote(\"{}\")",
                    row.get::<String, _>("name").replace('"', "\"\"")
                )
            })
            .collect::<Vec<_>>()
            .join(" || ',' || ");
        result.extend(
            sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(format!(
                "SELECT {expressions} AS contents FROM {quoted} ORDER BY contents"
            )))
            .fetch_all(&store.pool)
            .await?,
        );
    }
    Ok(result)
}

#[tokio::test]
async fn floor_opens_80_and_81_without_changing_schema_or_data() -> TestResult {
    for version in [80, 81] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.db");
        let store = SqliteStore::open_path(&path).await?;
        store
            .bootstrap_local_authority("aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa", "Host")
            .await?;
        store
            .create_room_for_local_operator(
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "general",
                "General",
            )
            .await?;
        let (_, identity) = guest(&store).await?;
        let now = Utc::now();
        let manager = store
            .authorize_local_room_manager(
                "general",
                agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await?;
        for token in [71, 72] {
            store
                .create_operator_pairing(
                    &crate::RoomManagerAuthority::Local(manager.clone()),
                    &[token; 32],
                    "https://host.example.test",
                    now,
                )
                .await?;
            store
                .redeem_operator_pairing(&[token; 32], &[73; 32], "https://host.example.test", now)
                .await?;
        }
        if version == 81 {
            install_v81(&store).await?;
            bind(
                &store,
                &identity
                    .user()
                    .unwrap_or_else(|| panic!("guest user"))
                    .user_id,
            )
            .await?;
            // Opaque extra member state models additive future tables; no C4b layout claim.
            sqlx::raw_sql("CREATE TABLE member_retention_probe (revision INTEGER, state TEXT, revoked INTEGER) STRICT;
                INSERT INTO member_retention_probe VALUES (7, 'banned', 1);")
                .execute(&store.pool).await?;
        }
        let before = snapshot(&store).await?;
        store.close().await?;
        let store = SqliteStore::open_path(&path).await?;
        assert_eq!(snapshot(&store).await?, before);
        verify_native_pairings(&store, now).await?;
        store.close().await?;
        let store = SqliteStore::open_path(&path).await?;
        for token in [71, 72] {
            assert!(
                store
                    .redeem_operator_pairing(
                        &[token; 32],
                        &[73; 32],
                        "https://host.example.test",
                        now
                    )
                    .await
                    .is_err()
            );
        }
        store.close().await?;
    }
    Ok(())
}

#[tokio::test]
async fn floor_rejects_newer_versions_and_incomplete_81() -> TestResult {
    for (version, partial) in [
        (81, false),
        (81, true),
        (82, false),
        (83, false),
        (999, false),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.db");
        let store = SqliteStore::open_path(&path).await?;
        if partial {
            install_v81(&store).await?;
            sqlx::query("ALTER TABLE central_identity_bindings DROP COLUMN created_at")
                .execute(&store.pool)
                .await?;
        }
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind(version.to_string())
            .execute(&store.pool)
            .await?;
        store.close().await?;
        let result = SqliteStore::open_path(&path).await;
        if version == 81 {
            assert!(matches!(
                result,
                Err(PersistenceError::InvalidSchemaVersion(_))
            ));
        } else {
            assert!(
                matches!(result, Err(PersistenceError::SchemaVersionMismatch { found, required: 80 })
                if found == version)
            );
        }
    }
    Ok(())
}

#[tokio::test]
async fn floor_rejects_malformed_81_structure() -> TestResult {
    let ddl = member_ddl();
    for (original, replacement) in [
        ("binding_id", "\"binding\"\"_id\""),
        (
            ") STRICT;",
            ") STRICT; CREATE TRIGGER extra_binding_trigger AFTER INSERT ON central_identity_bindings BEGIN SELECT 1; END;",
        ),
        (
            ") STRICT;",
            ") STRICT; CREATE UNIQUE INDEX extra_binding_unique ON central_identity_bindings(created_at);",
        ),
        (") STRICT", ")"),
        ("created_at INTEGER", "created_at TEXT"),
        ("person_id TEXT NOT NULL", "person_id TEXT"),
        (
            "binding_id TEXT PRIMARY KEY NOT NULL",
            "binding_id TEXT NOT NULL",
        ),
        ("UNIQUE(issuer, person_id),", ""),
        (",\n    UNIQUE(issuer, user_id)", ""),
        ("REFERENCES user_profiles(user_id) ON DELETE RESTRICT", ""),
        ("ON DELETE RESTRICT", "ON DELETE CASCADE"),
        ("PRIMARY KEY", "PRIMARY KEY ON CONFLICT REPLACE"),
        (
            "UNIQUE(issuer, person_id)",
            "UNIQUE(issuer, person_id) ON CONFLICT REPLACE",
        ),
        (
            "UNIQUE(issuer, user_id)",
            "UNIQUE(issuer, user_id) ON CONFLICT REPLACE",
        ),
        (") STRICT", ", CHECK(created_at > 0)) STRICT"),
        (
            "REFERENCES user_profiles(user_id)",
            "REFERENCES user_profiles(participant_id)",
        ),
        (
            "CREATE INDEX central_identity_bindings_user ON central_identity_bindings(user_id);",
            "",
        ),
        (
            "ON central_identity_bindings(user_id)",
            "ON central_identity_bindings(person_id)",
        ),
        (
            "ON central_identity_bindings(user_id);",
            "ON central_identity_bindings(user_id) WHERE created_at > 0;",
        ),
        (
            "ON central_identity_bindings(user_id)",
            "ON central_identity_bindings(user_id, (created_at + 1))",
        ),
    ] {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.db");
        let store = SqliteStore::open_path(&path).await?;
        let malformed = ddl.replace(original, replacement);
        assert_ne!(malformed, ddl);
        sqlx::raw_sql(sqlx::AssertSqlSafe(malformed))
            .execute(&store.pool)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '81' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await?;
        store.close().await?;
        assert!(
            SqliteStore::open_path(&path).await.is_err(),
            "accepted mutation: {original}"
        );
    }
    Ok(())
}

#[tokio::test]
async fn floor_denies_bound_human_admission_and_session_reuse() -> TestResult {
    let (store, now) = fixture().await;
    let (session, identity) = guest(&store).await?;
    install_v81(&store).await?;
    bind(
        &store,
        &identity
            .user()
            .unwrap_or_else(|| panic!("guest user"))
            .user_id,
    )
    .await?;
    let before = snapshot(&store).await?;
    for request_id in [
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
    ] {
        unsupported(
            store
                .admit_human(&prepared([2; 32], [3; 32], request_id, "Guest"), now)
                .await,
        );
    }
    unsupported(
        store
            .authorize_human_session(session.session_fingerprint())
            .await,
    );
    unsupported(store.revalidate_human_session_authorization(&session).await);
    unsupported(
        store
            .account_identity(AccountAuthority::BrowserDevice([3; 32]))
            .await,
    );
    assert_eq!(snapshot(&store).await?, before);
    Ok(())
}

#[tokio::test]
async fn floor_denies_bound_browser_one_use_first_and_replay() -> TestResult {
    for replay in [false, true] {
        let (store, now) = fixture().await;
        let (_, identity) = guest(&store).await?;
        insert_invite(&store, [11; 32], [12; 32], "one-use", 1, now).await;
        let request = prepared(
            [12; 32],
            [3; 32],
            "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
            "Guest",
        );
        if replay {
            admitted(store.admit_human(&request, now).await?);
        }
        install_v81(&store).await?;
        bind(
            &store,
            &identity
                .user()
                .unwrap_or_else(|| panic!("guest user"))
                .user_id,
        )
        .await?;
        let before = snapshot(&store).await?;
        for _ in 0..2 {
            unsupported(store.admit_human(&request, now).await);
            assert_eq!(snapshot(&store).await?, before);
        }
    }
    Ok(())
}

#[tokio::test]
async fn floor_denies_bound_recovery_issue() -> TestResult {
    let (store, _) = fixture().await;
    let (_, identity) = guest(&store).await?;
    install_v81(&store).await?;
    bind(
        &store,
        &identity
            .user()
            .unwrap_or_else(|| panic!("guest user"))
            .user_id,
    )
    .await?;
    let before = snapshot(&store).await?;
    unsupported(store.issue_guest_recovery_code(&identity).await);
    assert_eq!(snapshot(&store).await?, before);
    Ok(())
}

#[tokio::test]
async fn floor_denies_bound_recovery_fresh_and_exact_retry() -> TestResult {
    for replay in [false, true] {
        let (store, now) = fixture().await;
        let (_, identity) = guest(&store).await?;
        let code = fingerprint(&store.issue_guest_recovery_code(&identity).await?);
        let request = GuestRecoveryRequest {
            fingerprint: &code,
            device: &[4; 32],
            room_id: "general",
            client_id: "browser-client",
        };
        if replay {
            store.redeem_guest_recovery_code(&request, now).await?;
        }
        install_v81(&store).await?;
        bind(
            &store,
            &identity
                .user()
                .unwrap_or_else(|| panic!("guest user"))
                .user_id,
        )
        .await?;
        let before = snapshot(&store).await?;
        unsupported(store.redeem_guest_recovery_code(&request, now).await);
        assert_eq!(snapshot(&store).await?, before);
    }
    Ok(())
}

#[tokio::test]
async fn floor_denies_bound_device_binding_and_google_link() -> TestResult {
    let (store, _) = fixture().await;
    let (_, identity) = guest(&store).await?;
    let user_id = &identity
        .user()
        .unwrap_or_else(|| panic!("guest user"))
        .user_id;
    // A previously linked Google credential also must not mint another device.
    store
        .connect_google_account(&identity, &[9; 32], false)
        .await?;
    let new_device = store
        .account_identity(AccountAuthority::BrowserDevice([4; 32]))
        .await?;
    install_v81(&store).await?;
    bind(&store, user_id).await?;
    let before = snapshot(&store).await?;
    let mut tx = store.pool.begin().await?;
    unsupported(crate::account_identity::bind_account_device(&mut tx, user_id, &[5; 32]).await);
    tx.rollback().await?;
    unsupported(
        store
            .connect_google_account(&identity, &[9; 32], false)
            .await,
    );
    unsupported(
        store
            .connect_google_account(&new_device, &[9; 32], false)
            .await,
    );
    assert_eq!(snapshot(&store).await?, before);
    Ok(())
}

#[tokio::test]
async fn floor_keeps_unbound_anonymous_recovery_and_google_flow() -> TestResult {
    let (store, now) = fixture().await;
    install_v81(&store).await?;
    let (_, identity) = guest(&store).await?;
    let code = fingerprint(&store.issue_guest_recovery_code(&identity).await?);
    let request = GuestRecoveryRequest {
        fingerprint: &code,
        device: &[4; 32],
        room_id: "general",
        client_id: "browser-client",
    };
    let recovered = store.redeem_guest_recovery_code(&request, now).await?;
    let retry = store.redeem_guest_recovery_code(&request, now).await?;
    assert_eq!(recovered.session_bearer, retry.session_bearer);
    let device = store
        .account_identity(AccountAuthority::BrowserDevice([4; 32]))
        .await?;
    let linked = store
        .connect_google_account(&device, &[9; 32], false)
        .await?;
    assert_eq!(
        linked.user.user_id,
        identity
            .user()
            .unwrap_or_else(|| panic!("guest user"))
            .user_id
    );
    Ok(())
}

async fn verify_native_pairings(store: &SqliteStore, now: chrono::DateTime<Utc>) -> TestResult {
    use chrono::Duration;
    let origin = "https://host.example.test";
    let later = now + Duration::hours(2);
    for token in [71, 72] {
        let session = store
            .redeem_operator_pairing(&[token; 32], &[73; 32], origin, later)
            .await?;
        assert_eq!(session.authorization.expires_at(), None);
    }
    let times = sqlx::query_as::<_, (i64, i64)>(
        "SELECT session_expires_at, last_connected_at FROM operator_pairings ORDER BY token_fingerprint",
    ).fetch_all(&store.pool).await?;
    assert_eq!(times, vec![(0, later.timestamp()); 2]);
    assert!(
        store
            .redeem_operator_pairing(&[71; 32], &[73; 32], origin, later + Duration::days(30))
            .await
            .is_err()
    );
    // Failed idle use must not refresh the durable last-use timestamp.
    assert_eq!(sqlx::query_as::<_, (i64, i64)>(
        "SELECT session_expires_at, last_connected_at FROM operator_pairings ORDER BY token_fingerprint",
    ).fetch_all(&store.pool).await?, times);
    let owner = crate::ServerOwnerAuthority::LocalOperator;
    let devices = store.owner_device_sessions(&owner).await?;
    assert_eq!(devices.len(), 2);
    assert_eq!(
        store
            .revoke_owner_devices(&owner, Some(devices[0].session_id))
            .await?
            .revoked_count,
        1
    );
    assert_eq!(store.owner_device_sessions(&owner).await?.len(), 1);
    assert_eq!(
        store
            .revoke_owner_devices(&owner, None)
            .await?
            .revoked_count,
        1
    );
    for token in [71, 72] {
        assert!(
            store
                .redeem_operator_pairing(&[token; 32], &[73; 32], origin, later)
                .await
                .is_err()
        );
    }
    Ok(())
}
