//! Schema 92: rebuild the two whole-BLOB leaves without running DML after validation.
use crate::{PersistenceError, archive_schema, room_file_migration as migration, schema};
use sqlx::{Connection, SqliteConnection, SqlitePool};

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut expected = SqliteConnection::connect("sqlite::memory:").await?;
    let mut tx = pool.begin().await?;
    migration::require_source(&mut tx, 91).await?;
    let names = ["room_message_attachments", "room_connector_uploads"];
    for name in names {
        let replacement = format!("{name}_v92");
        if sqlx::query("SELECT 1 FROM sqlite_schema WHERE name=? COLLATE NOCASE")
            .bind(&replacement)
            .fetch_optional(&mut *tx)
            .await?
            .is_some()
        {
            return Err(migration::invalid(92, "replacement object"));
        }
        sqlx::query(sqlx::AssertSqlSafe(archive_schema::table_statement(
            91, name,
        )?))
        .execute(&mut expected)
        .await?;
        let expected_sql: String = sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE name=?")
            .bind(name)
            .fetch_one(&mut expected)
            .await?;
        let actual: Option<String> = sqlx::query_scalar("SELECT CASE WHEN length(sql)<=65536 THEN sql END FROM sqlite_schema WHERE name=? AND type='table'")
            .bind(name).fetch_optional(&mut *tx).await?.flatten();
        if actual.as_deref() != Some(&expected_sql) {
            return Err(migration::invalid(92, "source room file schema"));
        }
    }
    sqlx::query("UPDATE runtime_metadata SET value='92' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    for name in names {
        let replacement = format!("{name}_v92");
        let ddl = archive_schema::table_statement(92, name)?
            .replace("CREATE TABLE IF NOT EXISTS", "CREATE TABLE")
            .replace(&format!("\"{name}\""), &format!("\"{replacement}\""));
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {replacement} SELECT * FROM {name}"
        )))
        .execute(&mut *tx)
        .await?;
    }
    // Both sources remain available until every copied row/BLOB has been checked.
    for name in names {
        migration::require_equal_rows(&mut tx, name, &format!("{name}_v92"), "*", 92).await?;
    }
    migration::require_foreign_keys(&mut tx, 92).await?;
    // Only DDL from this point. The source graph has no incoming leaf references.
    for name in names {
        sqlx::query(sqlx::AssertSqlSafe(format!("DROP TABLE {name}")))
            .execute(&mut *tx)
            .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE {name}_v92 RENAME TO \"{name}\""
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
#[path = "room_file_schema_tests.rs"]
pub(crate) mod tests;
