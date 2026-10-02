use sqlx::{Row, SqlitePool};

use crate::PersistenceError;

pub const CURRENT_SCHEMA_VERSION: i64 = 73;

pub(crate) async fn validate_schema_version(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let stored = sqlx::query("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
        .fetch_optional(pool)
        .await?
        .map(|row| row.get::<String, _>("value"))
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion("missing".to_owned()))?;
    let found = stored
        .parse::<i64>()
        .ok()
        .filter(|version| *version >= 1)
        .ok_or_else(|| PersistenceError::InvalidSchemaVersion(stored.clone()))?;
    if !(70..=CURRENT_SCHEMA_VERSION).contains(&found) {
        return Err(PersistenceError::SchemaVersionMismatch {
            found,
            required: CURRENT_SCHEMA_VERSION,
        });
    }
    let server_id = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'server_id'",
    )
    .fetch_optional(pool)
    .await?
    .ok_or(PersistenceError::InvalidServerId)?;
    uuid::Uuid::parse_str(&server_id)
        .map(|_| ())
        .map_err(|_| PersistenceError::InvalidServerId)
}

// Called only after the existing host key and database authority have been verified.
// Additive upgrades retain existing product rows and never promote existing sessions.
pub(crate) async fn upgrade_schema(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version == "70" {
        sqlx::query(crate::message_attachments::connector::DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '71' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if version == "70" || version == "71" {
        sqlx::query("ALTER TABLE operator_pairings ADD COLUMN central_owner INTEGER NOT NULL DEFAULT 0 CHECK(central_owner IN (0, 1))")
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '72' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    if matches!(version.as_str(), "70" | "71" | "72") {
        sqlx::query(crate::server_owner_authority::GRANT_DDL)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '73' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::CURRENT_SCHEMA_VERSION;
    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn supported_upgrade_preserves_bootstrap_room_and_pairing_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        for previous in [70, 71, 72] {
            let directory = tempfile::tempdir()?;
            let path = directory.path().join("runtime.sqlite3");
            let store = SqliteStore::open_path(&path).await?;
            sqlx::query("UPDATE local_bootstrap_authority SET schema_revision = 70")
                .execute(&store.pool)
                .await?;
            let request_id = uuid::Uuid::new_v4().to_string();
            store
                .bootstrap_local_authority(&request_id, "Preserved host")
                .await?;
            store
                .create_room_for_local_operator(
                    &uuid::Uuid::new_v4().to_string(),
                    "preserved",
                    "Preserved room",
                )
                .await?;
            let before: String =
                sqlx::query_scalar("SELECT room_json FROM rooms WHERE room_id = 'preserved'")
                    .fetch_one(&store.pool)
                    .await?;
            let manager = store
                .authorize_local_room_manager(
                    "preserved",
                    agentsassemble_domain::LOCAL_OPERATOR_USER_ID,
                    agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await?;
            store
                .create_operator_pairing(
                    &manager,
                    &[31; 32],
                    "https://preserved.example.test",
                    chrono::Utc::now(),
                )
                .await?;
            if previous == 70 {
                sqlx::query("DROP TABLE room_connector_uploads")
                    .execute(&store.pool)
                    .await?;
            }
            if previous < 72 {
                sqlx::query("ALTER TABLE operator_pairings DROP COLUMN central_owner")
                    .execute(&store.pool)
                    .await?;
            }
            sqlx::query("DROP TABLE central_owner_grants")
                .execute(&store.pool)
                .await?;
            sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
                .bind(previous.to_string())
                .execute(&store.pool)
                .await?;
            drop(store);
            let reopened = SqliteStore::open_path(&path).await?;
            assert!(
                reopened
                    .bootstrap_local_authority(&request_id, "Preserved host")
                    .await?
                    .deduplicated
            );
            assert_eq!(
                before,
                sqlx::query_scalar::<_, String>(
                    "SELECT room_json FROM rooms WHERE room_id = 'preserved'"
                )
                .fetch_one(&reopened.pool)
                .await?
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_connector_uploads")
                    .fetch_one(&reopened.pool)
                    .await?,
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT central_owner FROM operator_pairings")
                    .fetch_one(&reopened.pool)
                    .await?,
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, String>(
                    "SELECT value FROM runtime_metadata WHERE key = 'schema_version'"
                )
                .fetch_one(&reopened.pool)
                .await?,
                CURRENT_SCHEMA_VERSION.to_string()
            );
            drop(reopened);
            SqliteStore::open_path(&path).await?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn older_schema_is_rejected_without_conversion() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("create current store: {error}"));
        sqlx::query("UPDATE runtime_metadata SET value = ? WHERE key = 'schema_version'")
            .bind("69")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("set older schema: {error}"));
        drop(store);

        assert!(matches!(
            SqliteStore::open_path(&path).await,
            Err(PersistenceError::SchemaVersionMismatch { found, required })
                if found == 69 && required == CURRENT_SCHEMA_VERSION
        ));
    }
}
