//! Schema 93: replace both whole-BLOB leaves atomically with metadata + pieces.
use crate::{PersistenceError, archive_schema, room_file_content::RoomFileContent, schema};
use sqlx::SqlitePool;

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    crate::room_file_migration::require_source(&mut tx, 92).await?;
    let mut copies = Vec::new();
    for kind in [RoomFileContent::Message, RoomFileContent::Connector] {
        let name = kind.parent();
        let replacement = format!("{name}_v93");
        let chunks = kind.chunks();
        for object in [&replacement, &chunks.to_owned()] {
            if sqlx::query("SELECT 1 FROM sqlite_schema WHERE name=? COLLATE NOCASE")
                .bind(object)
                .fetch_optional(&mut *tx)
                .await?
                .is_some()
            {
                return Err(PersistenceError::InvalidSchemaVersion(format!(
                    "93: replacement object exists: {object}"
                )));
            }
        }
        let expected = archive_schema::table_statement(92, name)?
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TABLE");
        let actual: Option<String> = sqlx::query_scalar("SELECT CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_schema WHERE name=? AND type='table'")
            .bind(name).fetch_optional(&mut *tx).await?.flatten();
        if actual.as_deref() != Some(&expected) {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "93: unexpected source schema: {name}"
            )));
        }
        let mut expected_indexes: Vec<String> = archive_schema::statements(92)?
            .into_iter()
            .filter(|s| s.starts_with("CREATE INDEX") && s.contains(&format!(" ON {name}(")))
            .map(|s| s.replace("CREATE INDEX IF NOT EXISTS", "CREATE INDEX"))
            .collect();
        expected_indexes.sort();
        let mut actual_indexes: Vec<Option<String>> = sqlx::query_scalar("SELECT CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_schema WHERE tbl_name=? AND type!='table' AND sql IS NOT NULL LIMIT ?")
            .bind(name).bind(i64::try_from(expected_indexes.len()+1).unwrap_or(i64::MAX)).fetch_all(&mut *tx).await?;
        actual_indexes.sort();
        if actual_indexes != expected_indexes.into_iter().map(Some).collect::<Vec<_>>() {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "93: unexpected source objects: {name}"
            )));
        }
    }
    sqlx::query("UPDATE runtime_metadata SET value='93' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    for kind in [RoomFileContent::Message, RoomFileContent::Connector] {
        let name = kind.parent();
        let replacement = format!("{name}_v93");
        let chunks = kind.chunks();
        let parent = schema::TABLES
            .iter()
            .find(|table| table.name == name)
            .ok_or_else(|| PersistenceError::InvalidSchemaVersion(name.into()))?;
        let ddl = parent
            .ddl
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TABLE")
            .replace(&format!("\"{name}\""), &format!("\"{replacement}\""));
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *tx)
            .await?;
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM pragma_table_info(?) WHERE name!='content' ORDER BY cid",
        )
        .bind(name)
        .fetch_all(&mut *tx)
        .await?;
        let columns = columns
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(",");
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {replacement}({columns}) SELECT {columns} FROM {name}"
        )))
        .execute(&mut *tx)
        .await?;
        let chunk_ddl = match kind {
            RoomFileContent::Message => crate::room_file_content::MESSAGE_DDL,
            RoomFileContent::Connector => crate::room_file_content::CONNECTOR_DDL,
        }
        .replace(&format!("\"{name}\""), &format!("\"{replacement}\""));
        sqlx::query(sqlx::AssertSqlSafe(chunk_ddl))
            .execute(&mut *tx)
            .await?;
        // substr(BLOB) never crosses the driver boundary. At most 800 rows per
        // source file; no Rust whole-file allocation, concatenation or spool.
        let piece = crate::room_file_content::PIECE_BYTES;
        let last = agentsassemble_domain::MAX_ROOM_FILE_BYTES.div_ceil(piece) - 1;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "WITH RECURSIVE pieces(sequence) AS (VALUES(0) UNION ALL SELECT sequence+1 FROM pieces WHERE sequence<{last})
             INSERT INTO {chunks}(attachment_id,sequence,content)
             SELECT attachment_id,sequence,substr(content,sequence*{piece}+1,{piece})
             FROM {name} CROSS JOIN pieces WHERE sequence*{piece}<size"
        ))).execute(&mut *tx).await?;
        copies.push((kind, replacement, columns));
    }
    for (kind, replacement, columns) in &copies {
        let name = kind.parent();
        let chunks = kind.chunks();
        crate::room_file_migration::require_equal_rows(&mut tx, name, replacement, columns, 93)
            .await?;
        kind.require_complete(&mut tx).await?;
        let mismatch: bool = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT EXISTS(SELECT 1 FROM {name} WHERE size!=length(content))
             OR EXISTS(SELECT 1 FROM {chunks} AS chunk JOIN {name} AS source USING(attachment_id)
             WHERE chunk.content IS NOT substr(source.content,chunk.sequence*65536+1,65536))"
        )))
        .fetch_one(&mut *tx)
        .await?;
        if mismatch {
            return Err(crate::room_file_migration::invalid(
                93,
                "room file chunk bytes",
            ));
        }
    }
    crate::room_file_migration::require_foreign_keys(&mut tx, 93).await?;
    // Only DDL after the byte-exact validation; no incoming source references.
    for (kind, replacement, _) in copies {
        let name = kind.parent();
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {replacement} RENAME TO \"{name}\""
        )))
        .execute(&mut *tx)
        .await?;
    }
    for statement in schema::statements()
        .filter(|s| s.starts_with("CREATE INDEX") && s.contains(" ON room_message_attachments("))
    {
        sqlx::query(statement).execute(&mut *tx).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "room_file_chunk_schema_tests.rs"]
mod tests;
