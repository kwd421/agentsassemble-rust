//! One derived result per retained attachment; NULL records original passthrough.
use crate::{PersistenceError, raster_preview::Preview};
use sqlx::{Connection, SqliteConnection, SqlitePool};

macro_rules! cache_ddl {
    ($table:literal, $parent:literal) => {
        concat!("CREATE TABLE \"", $table, "\" (\n",
            "    attachment_id TEXT NOT NULL PRIMARY KEY REFERENCES \"", $parent, "\"(attachment_id) ON DELETE CASCADE,\n",
            "    content_type TEXT, content BLOB,\n",
            "    CHECK((content_type IS NULL AND content IS NULL) OR\n",
            "        (content_type IS NOT NULL AND content_type IN ('image/jpeg','image/webp')\n",
            "        AND content IS NOT NULL AND typeof(content)='blob' AND length(content) BETWEEN 1 AND 262144))\n",
            ")")
    };
}
pub(crate) const MESSAGE_DDL: &str = cache_ddl!(
    "room_message_attachment_previews",
    "room_message_attachments"
);
pub(crate) const CONNECTOR_DDL: &str =
    cache_ddl!("room_connector_upload_previews", "room_connector_uploads");

pub(crate) async fn store(
    connection: &mut SqliteConnection,
    id: &str,
    preview: Option<Preview>,
    connector: bool,
) -> Result<(), PersistenceError> {
    let query = if connector {
        "INSERT INTO room_connector_upload_previews(attachment_id,content_type,content) VALUES(?,?,?)"
    } else {
        "INSERT INTO room_message_attachment_previews(attachment_id,content_type,content) VALUES(?,?,?)"
    };
    sqlx::query(query)
        .bind(id)
        .bind(preview.as_ref().map(|p| p.content_type))
        .bind(preview.map(|p| p.content))
        .execute(connection)
        .await?;
    Ok(())
}

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut expected = SqliteConnection::connect("sqlite::memory:").await?;
    let mut tx = pool.begin().await?;
    crate::room_file_migration::require_source(&mut tx, 93).await?;
    // Derive canonical SQL from frozen catalog, including indexes/triggers.
    // Validate all source objects before writing the version or creating cache.
    for name in [
        "room_message_attachments",
        "room_connector_uploads",
        "room_message_attachment_chunks",
        "room_connector_upload_chunks",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(crate::archive_schema::table_statement(
            93, name,
        )?))
        .execute(&mut expected)
        .await?;
        for ddl in crate::archive_schema::statements(93)?
            .into_iter()
            .filter(|s| s.starts_with("CREATE INDEX") && s.contains(&format!(" ON {name}(")))
        {
            sqlx::query(sqlx::AssertSqlSafe(ddl))
                .execute(&mut expected)
                .await?;
        }
        let query = "SELECT CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_schema WHERE tbl_name=? COLLATE NOCASE AND sql IS NOT NULL ORDER BY name";
        let canonical: Vec<Option<String>> = sqlx::query_scalar(query)
            .bind(name)
            .fetch_all(&mut expected)
            .await?;
        let actual: Vec<Option<String>> = sqlx::query_scalar(query)
            .bind(name)
            .fetch_all(&mut *tx)
            .await?;
        if actual != canonical {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "94: unexpected source objects: {name}"
            )));
        }
    }
    for name in [
        "room_message_attachment_previews",
        "room_connector_upload_previews",
    ] {
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name=? COLLATE NOCASE)",
        )
        .bind(name)
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "94: destination object exists: {name}"
            )));
        }
    }
    sqlx::query("UPDATE runtime_metadata SET value='94' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    // No original bytes are copied or changed in this migration. The validated
    // graph has no metadata references/triggers; retain complete original chunks.
    for kind in [
        crate::room_file_content::RoomFileContent::Message,
        crate::room_file_content::RoomFileContent::Connector,
    ] {
        kind.require_complete(&mut tx).await?;
    }
    crate::room_file_migration::require_foreign_keys(&mut tx, 94).await?;
    // Only DDL after final validation.
    for ddl in [MESSAGE_DDL, CONNECTOR_DDL] {
        sqlx::query(ddl).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "message_attachment_preview_schema_tests.rs"]
mod schema_tests;
