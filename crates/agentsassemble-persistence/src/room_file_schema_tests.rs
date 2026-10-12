use crate::{ServerOwnerAuthority, SqliteStore, backup_database};
use sqlx::{
    Connection, Row, SqliteConnection,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn rebuild_preserves_rows_custody_schema_and_retries_after_failure() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (source, _) = backup_database::archive_tests::archive(directory.path(), 91).await?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&source)
                .foreign_keys(true),
        )
        .await?;
    sqlx::query("INSERT INTO room_message_attachments SELECT 'ma_'||printf('%032x',2),room_id,user_id,NULL,'pending.bin','application/octet-stream',x'00ff0102',4,0,1,'pending',9223372036854775807 FROM rooms CROSS JOIN user_profiles LIMIT 1").execute(&pool).await?;
    sqlx::query("INSERT INTO room_connector_uploads SELECT 'ma_'||printf('%032x',3),room_id,participant_id,'connector.bin','application/octet-stream',x'ff000201',4,0,1,9223372036854775807 FROM participants LIMIT 1").execute(&pool).await?;
    let snapshot = directory.path().join("before.sqlite");
    sqlx::query("VACUUM INTO ?")
        .bind(snapshot.to_str().ok_or("path")?)
        .execute(&pool)
        .await?;
    let before: Vec<(String, String)> =
        sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await?;
    // Replacement objects must never be reused, even if empty and column-compatible.
    for kind in ["TABLE", "VIEW"] {
        let ddl = if kind == "TABLE" {
            "CREATE TABLE room_message_attachments_v92 AS SELECT * FROM room_message_attachments WHERE 0"
        } else {
            "CREATE VIEW room_message_attachments_v92 AS SELECT * FROM room_message_attachments"
        };
        sqlx::query(sqlx::AssertSqlSafe(ddl)).execute(&pool).await?;
        assert!(super::upgrade(&pool).await.is_err());
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP {kind} room_message_attachments_v92"
        )))
        .execute(&pool)
        .await?;
    }
    sqlx::query("ALTER TABLE room_connector_uploads ADD COLUMN hostile TEXT")
        .execute(&pool)
        .await?;
    assert!(super::upgrade(&pool).await.is_err());
    sqlx::query("ALTER TABLE room_connector_uploads DROP COLUMN hostile")
        .execute(&pool)
        .await?;
    // A same-typed reorder is also rejected, rather than copied positionally.
    let historical = crate::archive_schema::table_statement(91, "room_connector_uploads")?;
    let columns = "attachment_id,room_id,participant_id,filename,content_type,content,size,is_safe_image,created_at,expires_at";
    sqlx::query("CREATE TEMP TABLE reordered_rows AS SELECT * FROM room_connector_uploads")
        .execute(&pool)
        .await?;
    sqlx::query("DROP TABLE room_connector_uploads")
        .execute(&pool)
        .await?;
    sqlx::query(sqlx::AssertSqlSafe(historical.replace(
        "filename TEXT NOT NULL, content_type TEXT NOT NULL",
        "content_type TEXT NOT NULL, filename TEXT NOT NULL",
    )))
    .execute(&pool)
    .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO room_connector_uploads ({columns}) SELECT {columns} FROM reordered_rows"
    )))
    .execute(&pool)
    .await?;
    assert!(super::upgrade(&pool).await.is_err());
    sqlx::query("DROP TABLE room_connector_uploads")
        .execute(&pool)
        .await?;
    sqlx::query(sqlx::AssertSqlSafe(historical))
        .execute(&pool)
        .await?;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO room_connector_uploads ({columns}) SELECT {columns} FROM reordered_rows"
    )))
    .execute(&pool)
    .await?;
    sqlx::query("DROP TABLE reordered_rows")
        .execute(&pool)
        .await?;
    // Refuse the metadata trigger before a final version write can corrupt bytes.
    sqlx::query("CREATE TRIGGER fail_v92 AFTER UPDATE OF value ON RUNTIME_METADATA WHEN NEW.key='schema_version' AND NEW.value='92' BEGIN UPDATE room_message_attachments SET content=zeroblob(size); END").execute(&pool).await?;
    assert!(super::upgrade(&pool).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key='schema_version'"
        )
        .fetch_one(&pool)
        .await?,
        "91"
    );
    sqlx::query("DROP TRIGGER fail_v92").execute(&pool).await?;
    let after: Vec<(String, String)> =
        sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await?;
    assert_eq!(before, after);
    assert_all_file_rows(&pool, &snapshot).await?;
    super::upgrade(&pool).await?;
    assert_all_file_rows(&pool, &snapshot).await?;
    let mut upgraded = pool.acquire().await?;
    let mut fresh =
        backup_database::create_version_payload(&directory.path().join("fresh.sqlite"), 92).await?;
    backup_database::require_schema(&mut upgraded, &mut fresh, true).await?;
    backup_database::require_integrity(&mut upgraded).await?;
    // Custody remains enforced after replacement, including parent cascades.
    assert!(sqlx::query("UPDATE room_message_attachments SET pending_owner_user_id='missing' WHERE state='pending'").execute(&mut *upgraded).await.is_err());
    assert!(
        sqlx::query("UPDATE room_message_attachments SET event_seq=NULL WHERE state='bound'")
            .execute(&mut *upgraded)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM rooms")
        .execute(&mut *upgraded)
        .await?;
    for table in ["room_message_attachments", "room_connector_uploads"] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {table}"
            )))
            .fetch_one(&mut *upgraded)
            .await?,
            0
        );
    }
    drop(upgraded);
    pool.close().await;
    fresh.close().await?;
    Ok(())
}

async fn assert_all_file_rows(pool: &sqlx::SqlitePool, snapshot: &std::path::Path) -> TestResult {
    let mut connection = pool.acquire().await?;
    sqlx::query("ATTACH DATABASE ? AS original")
        .bind(snapshot.to_str().ok_or("path")?)
        .execute(&mut *connection)
        .await?;
    for table in ["room_message_attachments", "room_connector_uploads"] {
        for (a, b) in [("main", "original"), ("original", "main")] {
            let difference: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM (SELECT * FROM {a}.{table} EXCEPT SELECT * FROM {b}.{table})"
            )))
            .fetch_one(&mut *connection)
            .await?;
            assert_eq!(difference, 0, "{table}");
        }
    }
    sqlx::query("DETACH DATABASE original")
        .execute(&mut *connection)
        .await?;
    Ok(())
}

#[tokio::test]
async fn ceiling_file_exports_and_restores_without_relaxing_other_assets() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("live.sqlite")).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "general", "Room")
        .await?;
    let bytes: Vec<u8> = (0..agentsassemble_domain::MAX_ROOM_FILE_BYTES)
        .map(|i| (i % 251) as u8)
        .collect();
    sqlx::query("INSERT INTO room_message_attachments SELECT 'ma_'||printf('%032x',1),'general',user_id,NULL,'large.bin','application/octet-stream',?,0,1,'pending',9223372036854775807 FROM user_profiles LIMIT 1").bind(bytes.len() as i64).execute(&store.pool).await?;
    crate::room_file_content::RoomFileContent::Message
        .write(
            &mut *store.pool.acquire().await?,
            "ma_00000000000000000000000000000001",
            &bytes,
        )
        .await?;
    assert!(
        sqlx::query("UPDATE room_message_attachments SET size=52428801")
            .execute(&store.pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO profile_avatar_assets SELECT 'avatar',user_id,'avatar.png','image/png',zeroblob(10485761),10485761,'2026-10-12T00:00:00Z','current',NULL FROM user_profiles LIMIT 1").execute(&store.pool).await.is_err());
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(
            &ServerOwnerAuthority::LocalOperator,
            &directory.path().join("snapshot.sqlite"),
            &payload,
        )
        .await?;
    let (database_bytes, database_sha256) =
        crate::backup_codec::measure_payload(&mut std::fs::File::open(&payload)?)?;
    let mut source = backup_database::open_input(&payload).await?;
    let (rooms, messages, assets) = crate::restore_preparation::counts(&mut source).await?;
    let server_id = sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='server_id'")
        .fetch_one(&mut source)
        .await?;
    source.close().await?;
    let manifest = crate::backup_codec::BackupManifest {
        schema_version: crate::schema_version::CURRENT_SCHEMA_VERSION,
        server_id,
        server_name: "Owner".into(),
        issuer: "https://central.example".into(),
        owner_person_id: "owner".into(),
        source_epoch: "epoch".into(),
        source_key_fingerprint: "a".repeat(43),
        snapshot_at: chrono::Utc::now(),
        rooms,
        messages,
        assets,
        database_bytes,
        database_sha256,
    };
    let mut encoded = tempfile::tempfile()?;
    crate::backup_codec::encode(
        &mut std::fs::File::open(&payload)?,
        &mut encoded,
        &manifest,
        None,
        Default::default(),
    )?;
    std::io::Seek::rewind(&mut encoded)?;
    let decoded = directory.path().join("decoded.sqlite");
    let manifest =
        crate::backup_codec::decode(&mut encoded, &mut std::fs::File::create(&decoded)?, None)?;
    let destination = tempfile::tempdir()?;
    let mut job =
        crate::restore_job::RestoreJob::begin(&destination.path().join("runtime.sqlite3"))?;
    job.prepare(&decoded, manifest).await?;
    assert_eq!(
        job.journal().phase,
        crate::restore_job::RestorePhase::Prepared
    );
    let restored = job.stage().join("runtime.sqlite3");
    let mut connection = backup_database::open_input(&restored).await?;
    let row = sqlx::query("SELECT attachment_id,size FROM room_message_attachments")
        .fetch_one(&mut connection)
        .await?;
    assert_eq!(
        crate::room_file_content::RoomFileContent::Message
            .collect(&mut connection, row.get("attachment_id"), bytes.len())
            .await?,
        bytes
    );
    assert_eq!(row.get::<i64, _>("size"), 52428800);
    connection.close().await?;
    // New chunk archives retain the ordinary cell and aggregate metadata limits.
    let mut hostile =
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&payload)).await?;
    sqlx::query("UPDATE room_message_attachments SET filename=printf('%.*c',12000000,'x'),content_type=printf('%.*c',12000000,'x')").execute(&mut hostile).await?;
    hostile.close().await?;
    assert!(
        backup_database::reconstruct_payload(
            &payload,
            &directory.path().join("rejected.sqlite"),
            crate::schema_version::CURRENT_SCHEMA_VERSION
        )
        .await
        .is_err()
    );
    store.close().await?;
    Ok(())
}

// Restore genuine historical leaf DDL when a test simulates a pre-92 database.
pub(crate) async fn restore_v91(pool: &sqlx::SqlitePool) -> Result<(), sqlx::Error> {
    let mut tx = pool.begin().await?;
    for name in ["room_message_attachments", "room_connector_uploads"] {
        let kind = if name == "room_message_attachments" {
            crate::room_file_content::RoomFileContent::Message
        } else {
            crate::room_file_content::RoomFileContent::Connector
        };
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE TEMP TABLE fixture_rows AS SELECT *,NULL AS content FROM {name}"
        )))
        .execute(&mut *tx)
        .await?;
        let files: Vec<(String, i64)> =
            sqlx::query_as("SELECT attachment_id,size FROM fixture_rows")
                .fetch_all(&mut *tx)
                .await?;
        for (id, size) in files {
            let bytes = kind
                .collect(&mut tx, &id, size as usize)
                .await
                .map_err(|e| sqlx::Error::Protocol(e.to_string()))?;
            sqlx::query("UPDATE fixture_rows SET content=? WHERE attachment_id=?")
                .bind(bytes)
                .bind(id)
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {}", kind.chunks())))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(
            crate::archive_schema::table_statement(91, name).expect("frozen DDL"),
        ))
        .execute(&mut *tx)
        .await?;
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(name)
                .fetch_all(&mut *tx)
                .await?;
        let columns = columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(",");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {name}({columns}) SELECT {columns} FROM fixture_rows"
        )))
        .execute(&mut *tx)
        .await?;
        sqlx::query("DROP TABLE fixture_rows")
            .execute(&mut *tx)
            .await?;
    }
    for statement in crate::schema::statements()
        .filter(|s| s.starts_with("CREATE INDEX") && s.contains(" ON room_message_attachments("))
    {
        sqlx::query(statement).execute(&mut *tx).await?;
    }
    tx.commit().await
}
