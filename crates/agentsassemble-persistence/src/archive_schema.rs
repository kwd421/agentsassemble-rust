//! Immutable archive DDL, frozen from native export 9c3afbee (schema 87).
//! Changes describe exact exported objects, not live ALTER TABLE spellings.
use crate::{PersistenceError, backup_json::invalid};
use serde::Deserialize;
use sqlx::{Row, SqliteConnection};

pub(crate) const FLOOR: i64 = 87;

#[derive(Deserialize)]
struct Change {
    version: i64,
    remove: Vec<String>,
    add: Vec<String>,
}

pub(crate) fn supported(version: i64) -> bool {
    (FLOOR..=crate::schema_version::CURRENT_SCHEMA_VERSION).contains(&version)
}

pub(crate) fn statements(version: i64) -> Result<Vec<String>, PersistenceError> {
    if !supported(version) {
        return Err(invalid());
    }
    let mut ddl: Vec<String> = serde_json::from_str(include_str!("archive_schema_87.json"))?;
    let changes: Vec<Change> = serde_json::from_str(include_str!("archive_schema_changes.json"))?;
    for next in (FLOOR + 1)..=version {
        let change = changes
            .iter()
            .find(|c| c.version == next)
            .ok_or_else(invalid)?;
        for removed in &change.remove {
            let index = ddl.iter().position(|s| s == removed).ok_or_else(invalid)?;
            ddl.remove(index);
        }
        ddl.extend(change.add.iter().cloned());
    }
    Ok(ddl)
}

pub(crate) async fn version(source: &mut SqliteConnection) -> Result<i64, PersistenceError> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT CASE WHEN length(CAST(value AS BLOB))<=20 THEN value END
         FROM runtime_metadata WHERE key='schema_version'",
    )
    .fetch_optional(source)
    .await?
    .flatten();
    let value = value.ok_or_else(invalid)?;
    let version = value.parse::<i64>().map_err(|_| invalid())?;
    if !supported(version) || value != version.to_string() {
        return Err(invalid());
    }
    Ok(version)
}

// Read the trusted destination's table inventory, excluding FTS-owned shadows.
pub(crate) async fn tables(output: &mut SqliteConnection) -> Result<Vec<String>, PersistenceError> {
    Ok(sqlx::query("SELECT name FROM pragma_table_list WHERE schema='main' AND type IN ('table','virtual') AND name!='sqlite_schema' ORDER BY name")
        .fetch_all(output).await?.iter().map(|r| r.get("name")).collect())
}

pub(crate) fn table_statement(version: i64, name: &str) -> Result<String, PersistenceError> {
    statements(version)?
        .into_iter()
        .find(|ddl| {
            ddl.starts_with("CREATE TABLE ")
                && (ddl.contains(&format!(" {name} (")) || ddl.contains(&format!(" \"{name}\" (")))
        })
        .ok_or_else(invalid)
}
