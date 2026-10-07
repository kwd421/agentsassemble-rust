//! Additional transport custody on top of durable browser/device identity.
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::{PersistenceError, RoomSessionAuthorization, SqliteStore};

#[derive(Clone, PartialEq, Eq)]
pub struct SecureSessionBinding {
    pub client_key_fingerprint: [u8; 32],
    pub channel_id: String,
}

impl SecureSessionBinding {
    /// # Errors
    /// Rejects use outside the exact authenticated channel, including plaintext use.
    pub fn require_match(
        expected: Option<&Self>,
        presented: Option<&Self>,
    ) -> Result<(), PersistenceError> {
        if expected != presented {
            return Err(invalid());
        }
        Ok(())
    }
}

pub(crate) fn from_row(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<Option<SecureSessionBinding>, PersistenceError> {
    let key: Option<Vec<u8>> = row.try_get("secure_client_key_fingerprint")?;
    let channel: Option<String> = row.try_get("secure_channel_id")?;
    match (key, channel) {
        (None, None) => Ok(None),
        (Some(key), Some(channel_id)) if channel_id.len() == 43 => Ok(Some(SecureSessionBinding {
            client_key_fingerprint: key.try_into().map_err(|_| invalid())?,
            channel_id,
        })),
        _ => Err(invalid()),
    }
}

pub(crate) async fn bind_member(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    binding: Option<&SecureSessionBinding>,
) -> Result<(), PersistenceError> {
    if let Some(binding) = binding {
        sqlx::query("UPDATE human_room_sessions SET secure_client_key_fingerprint = ?, secure_channel_id = ? WHERE session_fingerprint = ?")
            .bind(binding.client_key_fingerprint.as_slice()).bind(&binding.channel_id)
            .bind(fingerprint.as_slice()).execute(&mut **tx).await?;
    }
    Ok(())
}

pub(crate) async fn require_member_binding(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    presented: Option<&SecureSessionBinding>,
) -> Result<(), PersistenceError> {
    let row = sqlx::query("SELECT secure_client_key_fingerprint, secure_channel_id FROM human_room_sessions WHERE session_fingerprint = ?")
        .bind(fingerprint.as_slice()).fetch_one(&mut **tx).await?;
    SecureSessionBinding::require_match(from_row(&row)?.as_ref(), presented)
}

impl SqliteStore {
    /// # Errors
    /// Rejects bearer or socket ticket use outside its admitted encrypted transport.
    pub async fn require_secure_room_transport(
        &self,
        session: &RoomSessionAuthorization,
        presented: Option<&SecureSessionBinding>,
    ) -> Result<(), PersistenceError> {
        match session {
            RoomSessionAuthorization::Human(human) => {
                self.require_secure_human_transport(human.session_fingerprint(), presented)
                    .await
            }
            RoomSessionAuthorization::Operator(operator) => {
                let expected = if operator.is_central_owner() {
                    self.owner_for_operator_session(operator)
                        .await?
                        .and_then(|owner| owner.binding().secure.clone())
                } else {
                    None
                };
                SecureSessionBinding::require_match(expected.as_ref(), presented)
            }
        }
    }

    /// # Errors
    /// Rejects an unmatched channel and propagates persistent authority failures.
    pub async fn require_secure_human_transport(
        &self,
        fingerprint: &[u8; 32],
        presented: Option<&SecureSessionBinding>,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        require_member_binding(&mut tx, fingerprint, presented).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Ends only this channel's custody. Call after close and after any accepted
    /// admission finishes, so a late durable commit can never revive authority.
    /// # Errors
    /// Propagates durable disconnect failure; the transport must remain closed.
    pub async fn disconnect_secure_channel(&self, channel: &str) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("UPDATE host_owner_sessions SET connected = 0 WHERE secure_channel_id = ?")
            .bind(channel)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE human_room_sessions SET state = 'ended' WHERE secure_channel_id = ?")
            .bind(channel)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }
}

pub(crate) async fn upgrade(pool: &SqlitePool, version: &str) -> Result<(), PersistenceError> {
    if !version.parse::<i64>().is_ok_and(|version| version < 86) {
        return Ok(());
    }
    let mut tx = pool.begin().await?;
    // Earlier upgrade steps may create a table using its current DDL. Add only
    // the two columns absent from that already-upgraded table, never product rows.
    for (table, definitions) in [
        (
            "host_owner_sessions",
            [
                "ALTER TABLE host_owner_sessions ADD COLUMN secure_client_key_fingerprint BLOB CHECK(secure_client_key_fingerprint IS NULL OR length(secure_client_key_fingerprint)=32)",
                "ALTER TABLE host_owner_sessions ADD COLUMN secure_channel_id TEXT CHECK(secure_channel_id IS NULL OR length(secure_channel_id)=43)",
            ],
        ),
        (
            "human_room_sessions",
            [
                "ALTER TABLE human_room_sessions ADD COLUMN secure_client_key_fingerprint BLOB CHECK(secure_client_key_fingerprint IS NULL OR length(secure_client_key_fingerprint)=32)",
                "ALTER TABLE human_room_sessions ADD COLUMN secure_channel_id TEXT CHECK(secure_channel_id IS NULL OR length(secure_channel_id)=43)",
            ],
        ),
    ] {
        let columns = sqlx::query("SELECT name FROM pragma_table_info(?)")
            .bind(table)
            .fetch_all(&mut *tx)
            .await?;
        for (name, ddl) in ["secure_client_key_fingerprint", "secure_channel_id"]
            .into_iter()
            .zip(definitions)
        {
            if !columns.iter().any(|row| row.get::<&str, _>("name") == name) {
                sqlx::query(ddl).execute(&mut *tx).await?;
            }
        }
    }
    sqlx::query("UPDATE runtime_metadata SET value='86' WHERE key='schema_version'")
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

fn invalid() -> PersistenceError {
    crate::account_identity::rejected(
        "secure_transport_required",
        "Use the admitted server connection.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn v85_upgrade_preserves_room_and_legacy_custody_columns()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path).await?;
        store
            .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
            .await?;
        let room = store
            .create_room_for_local_operator(
                &uuid::Uuid::new_v4().to_string(),
                "retained",
                "Retained",
            )
            .await?;
        for ddl in [
            "ALTER TABLE host_owner_sessions DROP COLUMN secure_client_key_fingerprint",
            "ALTER TABLE host_owner_sessions DROP COLUMN secure_channel_id",
            "ALTER TABLE human_room_sessions DROP COLUMN secure_client_key_fingerprint",
            "ALTER TABLE human_room_sessions DROP COLUMN secure_channel_id",
            "UPDATE runtime_metadata SET value='85' WHERE key='schema_version'",
        ] {
            sqlx::query(ddl).execute(&store.pool).await?;
        }
        store.close().await?;
        let reopened = SqliteStore::open_path(&path).await?;
        let rooms = reopened
            .list_room_directory_for_owner(&crate::ServerOwnerAuthority::LocalOperator, true)
            .await?
            .1;
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0].room.room_uid, room.room.room_uid);
        let version: String =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
                .fetch_one(&reopened.pool)
                .await?;
        assert_eq!(version, crate::CURRENT_SCHEMA_VERSION.to_string());
        reopened.close().await?;
        Ok(())
    }
}
