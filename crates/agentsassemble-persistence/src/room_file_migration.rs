//! Validate the mutation graph, without requiring live ALTER spellings to match archives.
use crate::{PersistenceError, archive_schema};
use sqlx::{Connection, SqliteConnection};

type Trigger = (String, String, String, Option<String>);
type Reference = (
    String,
    i64,
    i64,
    String,
    String,
    Option<String>,
    String,
    String,
    String,
);

pub(crate) async fn require_source(
    source: &mut SqliteConnection,
    version: i64,
) -> Result<(), PersistenceError> {
    let mut canonical = SqliteConnection::connect("sqlite::memory:").await?;
    for ddl in archive_schema::statements(version)? {
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut canonical)
            .await?;
    }
    // SQLite builds a trigger from the stored SQL whatever the row's `type` claims, so
    // any row whose SQL names the keyword is held to the canonical trigger set as well.
    let triggers = "SELECT type,name,tbl_name,CASE WHEN length(sql)<=65536 THEN sql END
        FROM main.sqlite_schema WHERE type='trigger' OR sql LIKE '%trigger%' ORDER BY name LIMIT ?";
    let expected: Vec<Trigger> = sqlx::query_as(triggers)
        .bind(i64::MAX)
        .fetch_all(&mut canonical)
        .await?;
    let actual: Vec<Trigger> = sqlx::query_as(triggers)
        .bind((expected.len() + 1) as i64)
        .fetch_all(&mut *source)
        .await?;
    if actual != expected {
        return Err(invalid(version, "trigger catalog"));
    }
    let metadata = "SELECT type,name,tbl_name,CASE WHEN length(sql)<=65536 THEN sql END
        FROM main.sqlite_schema WHERE tbl_name='runtime_metadata' COLLATE NOCASE ORDER BY name LIMIT ?";
    let expected: Vec<(String, String, String, Option<String>)> = sqlx::query_as(metadata)
        .bind(i64::MAX)
        .fetch_all(&mut canonical)
        .await?;
    let actual: Vec<(String, String, String, Option<String>)> = sqlx::query_as(metadata)
        .bind((expected.len() + 1) as i64)
        .fetch_all(&mut *source)
        .await?;
    if actual != expected {
        return Err(invalid(version, "runtime metadata catalog"));
    }
    // Every named schema row participates, whatever type it claims: SQLite builds a
    // table (and its foreign-key actions) from the stored SQL, not from the type column.
    let references = r#"SELECT s.name,f.id,f.seq,f."table",f."from",f."to",f.on_update,f.on_delete,f."match"
        FROM main.sqlite_schema s JOIN pragma_foreign_key_list(s.name,'main') f
        WHERE f."table" COLLATE NOCASE IN
        ('runtime_metadata','room_message_attachments','room_connector_uploads',
         'room_message_attachment_chunks','room_connector_upload_chunks',
         'room_message_attachment_previews','room_connector_upload_previews')
        ORDER BY s.name,f.id,f.seq LIMIT ?"#;
    let expected: Vec<Reference> = sqlx::query_as(references)
        .bind(i64::MAX)
        .fetch_all(&mut canonical)
        .await?;
    let actual: Vec<Reference> = sqlx::query_as(references)
        .bind((expected.len() + 1) as i64)
        .fetch_all(&mut *source)
        .await?;
    if actual != expected {
        return Err(invalid(version, "room file foreign-key graph"));
    }
    Ok(())
}

pub(crate) fn invalid(version: i64, reason: &str) -> PersistenceError {
    PersistenceError::InvalidSchemaVersion(format!("{version}: noncanonical {reason}"))
}

// Full row comparison keeps BLOBs inside SQLite and also proves custody/metadata.
pub(crate) async fn require_equal_rows(
    connection: &mut SqliteConnection,
    source: &str,
    replacement: &str,
    columns: &str,
    version: i64,
) -> Result<(), PersistenceError> {
    let columns = if columns == "*" {
        sqlx::query_scalar::<_, String>("SELECT name FROM pragma_table_info(?) ORDER BY cid")
            .bind(source)
            .fetch_all(&mut *connection)
            .await?
    } else {
        columns
            .split(',')
            .map(|name| name.trim_matches('"').to_owned())
            .collect()
    };
    let mismatch = columns
        .iter()
        .map(|name| format!("source.\"{name}\" IS NOT destination.\"{name}\""))
        .collect::<Vec<_>>()
        .join(" OR ");
    // Indexed identity joins avoid materializing whole BLOB sets in an EXCEPT temp tree.
    let different: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
        "SELECT EXISTS(SELECT 1 FROM {source} AS source
         LEFT JOIN {replacement} AS destination USING(attachment_id)
         WHERE destination.attachment_id IS NULL OR {mismatch})
         OR EXISTS(SELECT 1 FROM {replacement} AS destination
         LEFT JOIN {source} AS source USING(attachment_id) WHERE source.attachment_id IS NULL)"
    )))
    .fetch_one(connection)
    .await?;
    if different {
        return Err(invalid(version, "room file copy bytes or metadata"));
    }
    Ok(())
}

pub(crate) async fn require_foreign_keys(
    connection: &mut SqliteConnection,
    version: i64,
) -> Result<(), PersistenceError> {
    if sqlx::query("SELECT * FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(connection)
        .await?
        .is_some()
    {
        return Err(invalid(version, "room file foreign keys"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "room_file_migration_tests.rs"]
mod tests;
