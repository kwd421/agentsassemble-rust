//! Rebuild only the two room-file leaves; no tables reference either leaf.
use crate::{PersistenceError, schema};
use sqlx::{Connection, SqliteConnection, SqlitePool};

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    // Use the frozen pre-migration catalog as the sole source-schema authority.
    let mut expected = SqliteConnection::connect("sqlite::memory:").await?;
    let mut tx = pool.begin().await?;
    for name in ["room_message_attachments", "room_connector_uploads"] {
        let table = schema::TABLES
            .iter()
            .find(|table| table.name == name)
            .ok_or_else(|| PersistenceError::InvalidSchemaVersion(name.into()))?;
        let replacement = format!("{name}_v92");
        if sqlx::query("SELECT 1 FROM sqlite_schema WHERE name = ?")
            .bind(&replacement)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
        {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "92: replacement object exists: {replacement}"
            )));
        }
        let source_ddl = crate::archive_schema::table_statement(91, name)?;
        sqlx::query(sqlx::AssertSqlSafe(source_ddl.as_str()))
            .execute(&mut expected)
            .await?;
        let expected_sql: String =
            sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name = ?")
                .bind(name)
                .fetch_one(&mut expected)
                .await?;
        let actual_sql: Option<String> = sqlx::query_scalar("SELECT CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_schema WHERE name = ? AND type = 'table'")
            .bind(name).fetch_optional(&mut *tx).await?.flatten();
        if actual_sql.as_deref() != Some(&expected_sql) {
            return Err(PersistenceError::InvalidSchemaVersion(format!(
                "92: unexpected source schema: {name}"
            )));
        }
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) ORDER BY cid")
                .bind(name)
                .fetch_all(&mut expected)
                .await?;
        let columns = columns
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(",");
        let ddl = table
            .ddl
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TABLE")
            .replace(&format!("\"{name}\""), &format!("\"{replacement}\""));
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *tx)
            .await?;
        let count: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {name}")))
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {replacement} ({columns}) SELECT {columns} FROM {name}"
        )))
        .execute(&mut *tx)
        .await?;
        let copied: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT count(*) FROM {replacement}"
        )))
        .fetch_one(&mut *tx)
        .await?;
        if copied != count
            || sqlx::query("SELECT * FROM pragma_foreign_key_check(?) LIMIT 1")
                .bind(&replacement)
                .fetch_optional(&mut *tx)
                .await?
                .is_some()
        {
            return Err(PersistenceError::InvalidSchemaVersion(
                "92: room file copy failed validation".into(),
            ));
        }
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {replacement} RENAME TO \"{name}\""
        )))
        .execute(&mut *tx)
        .await?;
    }
    for statement in schema::statements().filter(|statement| {
        statement.starts_with("CREATE INDEX") && statement.contains(" ON room_message_attachments(")
    }) {
        sqlx::query(statement).execute(&mut *tx).await?;
    }
    if sqlx::query("SELECT * FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(&mut *tx)
        .await?
        .is_some()
    {
        return Err(PersistenceError::InvalidSchemaVersion(
            "92: room file foreign keys".into(),
        ));
    }
    sqlx::query("UPDATE runtime_metadata SET value='92' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "room_file_schema_tests.rs"]
pub(crate) mod tests;
