//! Rebuild only the two room-file leaves; no tables reference either leaf.
use crate::{PersistenceError, schema};
use sqlx::SqlitePool;

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    for name in ["room_message_attachments", "room_connector_uploads"] {
        let table = schema::TABLES
            .iter()
            .find(|table| table.name == name)
            .ok_or_else(|| PersistenceError::InvalidSchemaVersion(name.into()))?;
        let replacement = format!("{name}_v92");
        let ddl = table
            .ddl
            .replace(&format!("\"{name}\""), &format!("\"{replacement}\""));
        sqlx::query(sqlx::AssertSqlSafe(ddl))
            .execute(&mut *tx)
            .await?;
        let count: i64 =
            sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {name}")))
                .fetch_one(&mut *tx)
                .await?;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {replacement} SELECT * FROM {name}"
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
mod tests;
