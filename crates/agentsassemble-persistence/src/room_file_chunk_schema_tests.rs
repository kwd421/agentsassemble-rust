use crate::{backup_database, room_file_content::RoomFileContent};
use sqlx::{
    Connection, SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn genuine_v92_converts_exact_bytes_and_refuses_hostile_shapes_atomically() -> TestResult {
    let directory = tempfile::tempdir()?;
    let (source, _) = backup_database::archive_tests::archive(directory.path(), 92).await?;
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&source)
                .foreign_keys(true),
        )
        .await?;
    let message = (0..65553).map(|i| u8::try_from(i % 251).unwrap()).collect::<Vec<_>>();
    let connector = (0..agentsassemble_domain::MAX_ROOM_FILE_BYTES)
        .map(|i| u8::try_from(i % 239).unwrap())
        .collect::<Vec<_>>();
    sqlx::query("INSERT INTO room_message_attachments SELECT 'ma_'||printf('%032x',2),room_id,user_id,NULL,'pending.bin','application/octet-stream',?,?,0,1,'pending',9223372036854775807 FROM rooms CROSS JOIN user_profiles LIMIT 1")
        .bind(&message).bind(i64::try_from(message.len()).unwrap()).execute(&pool).await?;
    sqlx::query("INSERT INTO room_connector_uploads SELECT 'ma_'||printf('%032x',3),room_id,participant_id,'connector.bin','application/octet-stream',?,?,0,1,9223372036854775807 FROM participants LIMIT 1")
        .bind(&connector).bind(i64::try_from(connector.len()).unwrap()).execute(&pool).await?;
    let expected_quota: i64 = sqlx::query_scalar("SELECT sum(size) FROM (SELECT size FROM room_message_attachments UNION ALL SELECT size FROM room_connector_uploads)").fetch_one(&pool).await?;
    let objects: Vec<(String, String)> =
        sqlx::query_as("SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name")
            .fetch_all(&pool)
            .await?;
    for (create, drop) in [
        (
            "CREATE TABLE room_message_attachments_v93(value)",
            "DROP TABLE room_message_attachments_v93",
        ),
        (
            "CREATE VIEW room_connector_upload_chunks AS SELECT attachment_id FROM room_connector_uploads",
            "DROP VIEW room_connector_upload_chunks",
        ),
        (
            "ALTER TABLE room_connector_uploads ADD COLUMN hostile TEXT",
            "ALTER TABLE room_connector_uploads DROP COLUMN hostile",
        ),
        (
            "CREATE TRIGGER hostile AFTER INSERT ON room_connector_uploads BEGIN SELECT 1; END",
            "DROP TRIGGER hostile",
        ),
        // Refuse metadata triggers before they can mutate the rebuilt chunks.
        (
            "CREATE TRIGGER fail_v93 AFTER UPDATE OF value ON RUNTIME_METADATA WHEN NEW.key='schema_version' AND NEW.value='93' BEGIN DELETE FROM room_message_attachment_chunks; END",
            "DROP TRIGGER fail_v93",
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(create))
            .execute(&pool)
            .await?;
        assert!(super::upgrade(&pool).await.is_err(), "{create}");
        sqlx::query(sqlx::AssertSqlSafe(drop))
            .execute(&pool)
            .await?;
        assert_original(&pool, &message, &connector).await?;
        let after: Vec<(String, String)> = sqlx::query_as(
            "SELECT name,sql FROM sqlite_schema WHERE sql IS NOT NULL ORDER BY name",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(objects, after);
    }
    super::upgrade(&pool).await?;
    let mut db = pool.acquire().await?;
    for (kind, id, bytes) in [
        (
            RoomFileContent::Message,
            "ma_00000000000000000000000000000002",
            &message,
        ),
        (
            RoomFileContent::Connector,
            "ma_00000000000000000000000000000003",
            &connector,
        ),
    ] {
        assert_eq!(kind.collect(&mut db, id, bytes.len()).await?, *bytes);
        kind.require_complete(&mut db).await?;
    }
    let quota: i64 = sqlx::query_scalar("SELECT sum(size) FROM (SELECT size FROM room_message_attachments UNION ALL SELECT size FROM room_connector_uploads)").fetch_one(&mut *db).await?;
    assert_eq!(quota, expected_quota);
    let mut fresh =
        backup_database::create_version_payload(&directory.path().join("fresh.sqlite"), 93).await?;
    backup_database::require_schema(&mut db, &mut fresh, true).await?;
    backup_database::require_integrity(&mut db).await?;
    sqlx::query("DELETE FROM rooms").execute(&mut *db).await?;
    for kind in [RoomFileContent::Message, RoomFileContent::Connector] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                "SELECT count(*) FROM {}",
                kind.chunks()
            )))
            .fetch_one(&mut *db)
            .await?,
            0
        );
    }
    drop(db);
    pool.close().await;
    fresh.close().await?;
    Ok(())
}

async fn assert_original(pool: &SqlitePool, message: &[u8], connector: &[u8]) -> TestResult {
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key='schema_version'"
        )
        .fetch_one(pool)
        .await?,
        "92"
    );
    assert_eq!(sqlx::query_scalar::<_, Vec<u8>>("SELECT content FROM room_message_attachments WHERE attachment_id='ma_00000000000000000000000000000002'").fetch_one(pool).await?, message);
    assert_eq!(
        sqlx::query_scalar::<_, Vec<u8>>("SELECT content FROM room_connector_uploads")
            .fetch_one(pool)
            .await?,
        connector
    );
    Ok(())
}
