use crate::backup_database::archive_tests::archive;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn upgrade_refuses_noncanonical_objects_before_mutation_then_preserves_originals()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let (source, _) = archive(directory.path(), 93).await?;
    let pool: SqlitePool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(source)
                .foreign_keys(true),
        )
        .await?;
    let before: Vec<(String, i64)> = sqlx::query_as(
        "SELECT attachment_id,size FROM room_message_attachments ORDER BY attachment_id",
    )
    .fetch_all(&pool)
    .await?;
    for (create, remove) in [
        (
            "CREATE VIEW room_message_attachment_previews AS SELECT 1",
            "DROP VIEW room_message_attachment_previews",
        ),
        (
            "CREATE TRIGGER hostile AFTER UPDATE ON runtime_metadata BEGIN DELETE FROM room_message_attachments; END",
            "DROP TRIGGER hostile",
        ),
        (
            "CREATE INDEX hostile ON room_message_attachment_chunks(content)",
            "DROP INDEX hostile",
        ),
        (
            "ALTER TABLE room_connector_uploads ADD COLUMN hostile TEXT",
            "ALTER TABLE room_connector_uploads DROP COLUMN hostile",
        ),
    ] {
        sqlx::query(sqlx::AssertSqlSafe(create))
            .execute(&pool)
            .await?;
        assert!(super::upgrade(&pool).await.is_err(), "{create}");
        let version: String =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
                .fetch_one(&pool)
                .await?;
        assert_eq!(version, "93");
        let after: Vec<(String, i64)> = sqlx::query_as(
            "SELECT attachment_id,size FROM room_message_attachments ORDER BY attachment_id",
        )
        .fetch_all(&pool)
        .await?;
        assert_eq!(before, after);
        sqlx::query(sqlx::AssertSqlSafe(remove))
            .execute(&pool)
            .await?;
    }
    super::upgrade(&pool).await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&pool)
            .await?;
    assert_eq!(version, "94");
    crate::room_file_content::RoomFileContent::Message
        .require_complete(&mut *pool.acquire().await?)
        .await?;
    Ok(())
}
