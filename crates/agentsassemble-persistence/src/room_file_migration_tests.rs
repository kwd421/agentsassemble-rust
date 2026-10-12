use crate::{PersistenceError, backup_database, room_file_content::RoomFileContent};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn migrate(pool: &SqlitePool, version: i64) -> Result<(), PersistenceError> {
    match version {
        91 => crate::room_file_schema::upgrade(pool).await,
        92 => crate::room_file_chunk_schema::upgrade(pool).await,
        93 => crate::message_attachment_previews::upgrade(pool).await,
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn every_room_file_migration_refuses_indirect_cascades_and_future_table_triggers()
-> TestResult {
    for version in 91..=93 {
        let directory = tempfile::tempdir()?;
        let (path, _) = backup_database::archive_tests::archive(directory.path(), version).await?;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(
                SqliteConnectOptions::new()
                    .filename(path)
                    .foreign_keys(true),
            )
            .await?;
        let content_table = if version == 93 {
            "room_message_attachment_chunks"
        } else {
            "room_message_attachments"
        };
        let original: Vec<(String, Vec<u8>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT attachment_id,content FROM {content_table} ORDER BY attachment_id"
        )))
        .fetch_all(&pool)
        .await?;
        sqlx::raw_sql("CREATE UNIQUE INDEX hostile_metadata ON runtime_metadata(key,value);
            CREATE TABLE helper(k TEXT,v TEXT, FOREIGN KEY(k,v) REFERENCES runtime_metadata(key,value) ON UPDATE CASCADE);
            INSERT INTO helper SELECT key,value FROM runtime_metadata WHERE key='schema_version';")
            .execute(&pool).await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE TRIGGER hostile AFTER UPDATE ON helper BEGIN UPDATE {content_table} SET content=zeroblob(length(content)); END"
        ))).execute(&pool).await?;
        assert!(matches!(
            migrate(&pool, version).await,
            Err(PersistenceError::InvalidSchemaVersion(_))
        ));
        sqlx::query("DROP TRIGGER hostile").execute(&pool).await?;
        // The helper FK and noncanonical metadata index must also refuse without a trigger.
        assert!(migrate(&pool, version).await.is_err());
        sqlx::query("DROP INDEX hostile_metadata")
            .execute(&pool)
            .await?;
        assert!(migrate(&pool, version).await.is_err()); // Canonical metadata, extra incoming FK.
        sqlx::query("DROP TABLE helper").execute(&pool).await?;
        // An attacker editing sqlite_schema can attach a trigger to a future table.
        // Keep the loaded schema unchanged; the migration must inspect the catalog itself.
        sqlx::query("PRAGMA writable_schema=ON")
            .execute(&pool)
            .await?;
        let future = if version == 93 {
            "room_message_attachment_previews".into()
        } else {
            format!("room_message_attachments_v{}", version + 1)
        };
        sqlx::query("INSERT INTO sqlite_schema(type,name,tbl_name,rootpage,sql) VALUES('trigger','smuggled',?,0,?)")
            .bind(&future).bind(format!("CREATE TRIGGER smuggled AFTER INSERT ON {future} BEGIN DELETE FROM {content_table}; END"))
            .execute(&pool).await?;
        sqlx::query("PRAGMA writable_schema=OFF")
            .execute(&pool)
            .await?;
        assert!(migrate(&pool, version).await.is_err());
        sqlx::query("PRAGMA writable_schema=ON")
            .execute(&pool)
            .await?;
        sqlx::query("DELETE FROM sqlite_schema WHERE name='smuggled'")
            .execute(&pool)
            .await?;
        sqlx::query("PRAGMA writable_schema=OFF")
            .execute(&pool)
            .await?;
        // Removing or altering a legitimate trigger is as noncanonical as adding one.
        let canonical: String = sqlx::query_scalar(
            "SELECT sql FROM sqlite_schema WHERE name='room_message_search_delete'",
        )
        .fetch_one(&pool)
        .await?;
        sqlx::query("DROP TRIGGER room_message_search_delete")
            .execute(&pool)
            .await?;
        assert!(migrate(&pool, version).await.is_err());
        sqlx::query("CREATE TRIGGER room_message_search_delete AFTER DELETE ON room_message_search_records BEGIN SELECT 1; END")
            .execute(&pool).await?;
        assert!(migrate(&pool, version).await.is_err());
        sqlx::query("DROP TRIGGER room_message_search_delete")
            .execute(&pool)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(canonical))
            .execute(&pool)
            .await?;
        let actual: Vec<(String, Vec<u8>)> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT attachment_id,content FROM {content_table} ORDER BY attachment_id"
        )))
        .fetch_all(&pool)
        .await?;
        assert_eq!(actual, original);
        let stored: String =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(stored, version.to_string());
        migrate(&pool, version).await?; // Refusals remain retryable with genuine objects.
        pool.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn oldest_live_schema_runs_real_upgrade_stages_and_preserves_every_file_byte() -> TestResult {
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect("sqlite::memory:")
        .await?;
    sqlx::raw_sql(sqlx::AssertSqlSafe(include_str!("room_file_v70.sql")))
        .execute(&pool)
        .await?;
    sqlx::raw_sql("INSERT INTO runtime_metadata VALUES('schema_version','70');
        INSERT INTO rooms VALUES('room','{}','{}');
        INSERT INTO user_profiles VALUES('user','participant','{}');
        INSERT INTO participants VALUES('room','participant','{}');
        INSERT INTO room_message_attachments VALUES('ma_00000000000000000000000000000001','room','user',NULL,'old.bin','application/octet-stream',zeroblob(65553),65553,0,1,'pending',9223372036854775807);
        UPDATE room_message_attachments SET content=CAST(x'00ff01' || substr(content,4) AS BLOB);
        CREATE TEMP TABLE original_message AS SELECT * FROM room_message_attachments;")
        .execute(&pool).await?;
    // This is the real live upgrade owner, including all ALTER/rebuild stages from 70.
    crate::schema_version::upgrade_schema(&pool).await?;
    let mut db = pool.acquire().await?;
    let current: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut *db)
            .await?;
    assert_eq!(current, crate::CURRENT_SCHEMA_VERSION.to_string());
    // These live ALTER spellings really differ from fresh archive DDL; they are legitimate.
    let pairings: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name='operator_pairings'")
            .fetch_one(&mut *db)
            .await?;
    assert_ne!(
        pairings,
        crate::archive_schema::table_statement(94, "operator_pairings")?
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TABLE")
    );
    for kind in [RoomFileContent::Message, RoomFileContent::Connector] {
        kind.require_complete(&mut db).await?;
    }
    let metadata_columns = "attachment_id,room_id,pending_owner_user_id,event_seq,filename,content_type,size,is_safe_image,created_at,state,expires_at";
    super::require_equal_rows(
        &mut db,
        "original_message",
        "room_message_attachments",
        metadata_columns,
        94,
    )
    .await?;
    let changed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM room_message_attachment_chunks c JOIN original_message o USING(attachment_id) WHERE c.content IS NOT substr(o.content,c.sequence*65536+1,65536))")
        .fetch_one(&mut *db).await?;
    assert!(!changed);
    // Connector uploads first exist at 71. Historical DDL reaches HEAD unchanged in semantics.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM room_message_attachment_chunks")
            .fetch_one(&mut *db)
            .await?,
        2
    );
    Ok(())
}
