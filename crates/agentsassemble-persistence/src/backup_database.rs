//! Hostile input never supplies executable schema. Only locally owned DDL creates
//! the output; rows cross a typed field boundary before insertion.
use crate::{
    PersistenceError,
    backup_json::invalid,
    backup_policy::{self, Policy},
};
use futures_util::TryStreamExt;
use sqlx::{
    Column, Connection, Row, SqliteConnection, TypeInfo, ValueRef,
    sqlite::{SqliteConnectOptions, SqliteRow},
};
use std::{collections::BTreeMap, path::Path};

type Object = (String, String, String, Option<String>);

async fn objects(connection: &mut SqliteConnection) -> Result<Vec<Object>, PersistenceError> {
    Ok(
        sqlx::query_as("SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name")
            .fetch_all(connection)
            .await?,
    )
}

async fn columns(
    connection: &mut SqliteConnection,
    table: &str,
) -> Result<Vec<(String, String, i64)>, PersistenceError> {
    Ok(
        sqlx::query("SELECT name,type,hidden FROM pragma_table_xinfo(?) ORDER BY cid")
            .bind(table)
            .fetch_all(connection)
            .await?
            .iter()
            .map(|row| (row.get("name"), row.get("type"), row.get("hidden")))
            .collect(),
    )
}

pub(crate) async fn open_input(path: &Path) -> Result<SqliteConnection, PersistenceError> {
    // SQLx enables no loadable extensions unless explicitly configured.
    Ok(SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .row_buffer_size(1)
            .pragma("trusted_schema", "OFF")
            .pragma("query_only", "ON"),
    )
    .await?)
}

pub(crate) async fn create_payload(path: &Path) -> Result<SqliteConnection, PersistenceError> {
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(PersistenceError::HostIdentityFile)?;
    crate::private_fs::secure_file(&file, path).map_err(PersistenceError::HostIdentityFile)?;
    drop(file);
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .foreign_keys(false)
            .pragma("trusted_schema", "OFF"),
    )
    .await?;
    for statement in crate::schema::statements() {
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut connection)
            .await?;
    }
    Ok(connection)
}

pub(crate) async fn require_schema(
    source: &mut SqliteConnection,
    output: &mut SqliteConnection,
    hostile: bool,
) -> Result<(), PersistenceError> {
    let supplied = objects(source).await?;
    let expected = objects(output).await?;
    if hostile {
        if supplied != expected {
            return Err(invalid());
        }
    } else if supplied
        .iter()
        .map(|o| (&o.0, &o.1, &o.2))
        .ne(expected.iter().map(|o| (&o.0, &o.1, &o.2)))
    {
        return Err(invalid());
    }
    for table in crate::schema::tables() {
        backup_policy::policy(table.name)?;
        if columns(source, table.name).await? != columns(output, table.name).await? {
            return Err(invalid());
        }
    }
    Ok(())
}

#[derive(Clone)]
enum Cell {
    Null,
    Text(String),
    Integer(i64),
    Real(f64),
    Blob(Vec<u8>),
}
impl Cell {
    fn text(&self) -> Option<&str> {
        if let Self::Text(s) = self {
            Some(s)
        } else {
            None
        }
    }
}

fn row_values(row: &SqliteRow) -> Result<BTreeMap<String, Cell>, PersistenceError> {
    row.columns()
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let raw = row.try_get_raw(index)?;
            let value = if raw.is_null() {
                Cell::Null
            } else {
                match raw.type_info().name() {
                    "TEXT" => Cell::Text(row.try_get(index)?),
                    "INTEGER" | "BOOLEAN" => Cell::Integer(row.try_get(index)?),
                    "REAL" => Cell::Real(row.try_get(index)?),
                    "BLOB" => Cell::Blob(row.try_get(index)?),
                    _ => return Err(invalid()),
                }
            };
            Ok((column.name().to_owned(), value))
        })
        .collect()
}

fn sanitize(table: &str, values: &mut BTreeMap<String, Cell>) -> Result<bool, PersistenceError> {
    if table == "runtime_metadata" {
        return Ok(matches!(
            values.get("key").and_then(Cell::text),
            Some("schema_owner" | "schema_version" | "server_id")
        ));
    }
    // One-use pairing credentials already exchanged for sessions cannot be copied.
    if matches!(table, "room_attendee_invites" | "room_connector_invites")
        && !matches!(values.get("session_fingerprint"), Some(Cell::Null))
    {
        return Ok(false);
    }
    if table == "room_attendee_invites"
        && !matches!(values.get("parent_fingerprint"), Some(Cell::Null))
    {
        if !matches!(
            values.get("restore_parent_kind").and_then(Cell::text),
            Some("human" | "operator")
        ) {
            return Err(invalid());
        }
        values.insert("parent_fingerprint".into(), Cell::Null);
    }

    if table == "member_admissions" {
        use ring::rand::{SecureRandom, SystemRandom};
        let mut seed = vec![0; 32];
        SystemRandom::new()
            .fill(&mut seed)
            .map_err(|_| PersistenceError::HostIdentityEntropy)?;
        values.insert("session_key".into(), Cell::Blob(seed));
    }
    for (column, cell) in values {
        if column.ends_with("_json")
            && let Cell::Text(text) = cell
        {
            *text = backup_policy::json(table, column, text)?;
        }
    }
    Ok(true)
}

pub(crate) async fn reconstruct(
    source: &mut SqliteConnection,
    output: &mut SqliteConnection,
    hostile: bool,
) -> Result<(), PersistenceError> {
    require_schema(source, output, hostile).await?;
    let mut tx = output.begin().await?;
    for table in crate::schema::tables() {
        if backup_policy::policy(table.name)? == Policy::Ephemeral {
            if hostile
                && sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(format!(
                    "SELECT count(*) FROM \"{}\"",
                    table.name
                )))
                .fetch_one(&mut *source)
                .await?
                    != 0
            {
                return Err(invalid());
            }
            continue;
        }
        let fields = columns(source, table.name)
            .await?
            .into_iter()
            .filter(|c| c.2 == 0)
            .map(|c| format!("\"{}\"", c.0))
            .collect::<Vec<_>>()
            .join(",");
        let oversized = fields
            .split(',')
            .map(|column| format!("length(CAST({column} AS BLOB)) > 16777216"))
            .collect::<Vec<_>>()
            .join(" OR ");
        let oversized = format!(
            "SELECT EXISTS(SELECT 1 FROM \"{}\" WHERE {oversized})",
            table.name
        );
        if sqlx::query_scalar::<_, bool>(sqlx::AssertSqlSafe(oversized))
            .fetch_one(&mut *source)
            .await?
        {
            return Err(invalid());
        }
        let fields = if table.name == "room_attendee_invites" && !hostile {
            fields.replace("\"restore_parent_kind\"", "CASE WHEN parent_fingerprint IS NULL THEN restore_parent_kind ELSE (SELECT group_concat(kind) FROM (SELECT 'human' AS kind FROM human_room_sessions WHERE session_fingerprint=room_attendee_invites.parent_fingerprint UNION ALL SELECT 'operator' FROM operator_pairings WHERE session_fingerprint=room_attendee_invites.parent_fingerprint)) END AS restore_parent_kind")
        } else {
            fields
        };
        let query = format!("SELECT {fields} FROM \"{}\"", table.name);
        let mut rows = sqlx::query(sqlx::AssertSqlSafe(query)).fetch(&mut *source);
        while let Some(row) = rows.try_next().await? {
            let mut values = row_values(&row)?;
            if !sanitize(table.name, &mut values)? {
                continue;
            }
            let names = values
                .keys()
                .map(|n| format!("\"{n}\""))
                .collect::<Vec<_>>()
                .join(",");
            let parameters = vec!["?"; values.len()].join(",");
            let statement = format!(
                "INSERT INTO \"{}\" ({names}) VALUES ({parameters})",
                table.name
            );
            let mut insert = sqlx::query(sqlx::AssertSqlSafe(statement));
            for cell in values.into_values() {
                insert = match cell {
                    Cell::Null => insert.bind(Option::<String>::None),
                    Cell::Text(v) => insert.bind(v),
                    Cell::Integer(v) => insert.bind(v),
                    Cell::Real(v) => insert.bind(v),
                    Cell::Blob(v) => insert.bind(v),
                };
            }
            insert.execute(&mut *tx).await?;
        }
    }
    if !sqlx::query("PRAGMA foreign_key_check")
        .fetch_all(&mut *tx)
        .await?
        .is_empty()
    {
        return Err(invalid());
    }
    let integrity: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut *tx)
        .await?;
    if integrity != "ok" {
        return Err(invalid());
    }
    tx.commit().await?;
    Ok(())
}

/// Reconstruct a validated payload into an exclusive, newly created staging DB.
/// The caller owns private staging and must not activate either path on failure.
/// # Errors
/// Rejects schema objects, columns, fields or authority state outside v1 policy.
pub async fn reconstruct_payload(input: &Path, output: &Path) -> Result<(), PersistenceError> {
    let mut source = open_input(input).await?;
    let mut target = create_payload(output).await?;
    reconstruct(&mut source, &mut target, true).await?;
    source.close().await?;
    target.close().await?;
    Ok(())
}

impl crate::SqliteStore {
    /// Snapshot the authorized live database and reconstruct a sanitized payload.
    /// Both paths must be in caller-owned private staging, never a sync folder.
    /// # Errors
    /// Rejects stale owner authority, existing paths or unsupported product data.
    pub async fn export_backup_payload(
        &self,
        authority: &crate::ServerOwnerAuthority,
        snapshot: &Path,
        output: &Path,
    ) -> Result<chrono::DateTime<chrono::Utc>, PersistenceError> {
        if self.database_identity.is_none() {
            return Err(invalid());
        }
        let mut connection = self.pool.acquire().await?;
        let mut tx = connection.begin().await?;
        authority.revalidate(&mut tx).await?;
        tx.commit().await?;
        let file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(snapshot)
            .map_err(PersistenceError::HostIdentityFile)?;
        crate::private_fs::secure_file(&file, snapshot)
            .map_err(PersistenceError::HostIdentityFile)?;
        drop(file);
        let path = snapshot.to_str().ok_or_else(invalid)?;
        let snapshot_at = chrono::Utc::now();
        sqlx::query("VACUUM INTO ?")
            .bind(path)
            .execute(&mut *connection)
            .await?;
        drop(connection);
        let mut source = open_input(snapshot).await?;
        let mut target = create_payload(output).await?;
        reconstruct(&mut source, &mut target, false).await?;
        source.close().await?;
        target.close().await?;
        Ok(snapshot_at)
    }
}

#[cfg(test)]
#[path = "backup_database_tests.rs"]
mod tests;

/// Read counts from this module's freshly reconstructed payload.
/// # Errors
/// Rejects unreadable payload metadata; no runtime or migrations are opened.
pub async fn payload_counts(path: &Path) -> Result<(u64, u64, u64), PersistenceError> {
    let mut connection = open_input(path).await?;
    let counts = crate::restore_preparation::counts(&mut connection).await?;
    connection.close().await?;
    Ok(counts)
}
