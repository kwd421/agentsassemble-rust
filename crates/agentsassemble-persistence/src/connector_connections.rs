//! An external AI joined through this host's own MCP endpoint keeps its seat
//! across a restart. The MCP hub lives in memory, so after a restart the AI's
//! private connection ID is unknown; this table remembers only a hash of that ID
//! and the invitation it joined with. When the AI calls again with the same ID,
//! the host derives the same session credential from the stored invitation and
//! the AI is back in its own seat, with its role and profile. An AI that lost
//! its connection ID entirely gets back in through a seat invitation the owner
//! makes for that seat.
use chrono::{DateTime, Duration, Utc};
use sqlx::Row;
use uuid::Uuid;

use crate::{
    PersistenceError, SqliteStore,
    connector_admission::{TTL, parse_uuid, timestamp},
    connector_session::authorize_in,
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS connector_connections (connection_hash BLOB NOT NULL PRIMARY KEY CHECK(length(connection_hash) = 32), invite_id TEXT NOT NULL REFERENCES room_connector_invites(invite_id) ON DELETE CASCADE, invitation_identity BLOB NOT NULL CHECK(length(invitation_identity) = 32), display_name TEXT NOT NULL, created_at INTEGER NOT NULL) STRICT";

/// A "reconnect" invitation the owner made for an existing external AI seat:
/// joining with it takes that seat back instead of adding a participant.
pub(crate) const SEAT_DDL: &str = "CREATE TABLE IF NOT EXISTS connector_seat_invites (invite_id TEXT NOT NULL PRIMARY KEY REFERENCES room_connector_invites(invite_id) ON DELETE CASCADE, participant_id TEXT NOT NULL) STRICT";

pub(crate) const TABLES: &[crate::schema::TableDefinition] = &[
    crate::schema::TableDefinition {
        name: "connector_connections",
        ddl: DDL,
        infrastructure: false,
    },
    crate::schema::TableDefinition {
        name: "connector_seat_invites",
        ddl: SEAT_DDL,
        infrastructure: false,
    },
];

/// An external AI whose session ended stays listed, as detached, this long so
/// the owner can make it a reconnect invitation.
pub(crate) const AWAY_LISTING_WINDOW: Duration = Duration::days(7);

/// Secret-bearing result for the in-process MCP hub only; neither Debug nor Serialize.
pub struct ResumedConnector {
    pub session_bearer: String,
    pub room_id: String,
    pub room_uid: Uuid,
    pub participant_id: String,
    pub invitation_identity: [u8; 32],
    pub display_name: String,
}

impl SqliteStore {
    /// Remembers a joined connection under the hash of its private ID.
    ///
    /// # Errors
    /// Storage failures.
    pub async fn remember_connector_connection(
        &self,
        connection_hash: &[u8; 32],
        session_fingerprint: &[u8; 32],
        invitation_identity: &[u8; 32],
        display_name: &str,
        now: DateTime<Utc>,
    ) -> Result<(), PersistenceError> {
        // Rows of ended connections are never resumed again.
        sqlx::query("DELETE FROM connector_connections WHERE invite_id IN (SELECT invite_id FROM room_connector_invites WHERE revoked = 1 OR session_expires_at IS NULL OR session_expires_at <= ?)")
            .bind(now.timestamp_micros())
            .execute(&self.pool)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO connector_connections(connection_hash, invite_id, invitation_identity, display_name, created_at) SELECT ?, invite_id, ?, ?, ? FROM room_connector_invites WHERE session_fingerprint = ? AND revoked = 0")
            .bind(connection_hash.as_slice())
            .bind(invitation_identity.as_slice())
            .bind(display_name)
            .bind(now.timestamp_micros())
            .bind(session_fingerprint.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Restores a remembered connection whose session has not ended: its expiry
    /// moves a full lifetime out, and the same credential is derived from its
    /// invitation. A host restart therefore never revives an ended session.
    /// `None` when the ID is unknown or the seat has ended.
    ///
    /// # Errors
    /// Storage failures and inconsistent stored state.
    pub async fn resume_connector_connection(
        &self,
        connection_hash: &[u8; 32],
        now: DateTime<Utc>,
    ) -> Result<Option<ResumedConnector>, PersistenceError> {
        // An unknown ID is the common case on the public endpoint: answer it
        // with a read, never a write transaction.
        let known: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM connector_connections WHERE connection_hash = ?)",
        )
        .bind(connection_hash.as_slice())
        .fetch_one(&self.pool)
        .await?;
        if !known {
            return Ok(None);
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let Some(row) = sqlx::query("SELECT c.invitation_identity, c.display_name, i.invite_id, i.room_id, i.room_uid, i.participant_id, i.token_fingerprint, i.session_fingerprint, i.session_expires_at FROM connector_connections c JOIN room_connector_invites i ON i.invite_id = c.invite_id WHERE c.connection_hash = ? AND i.revoked = 0 AND i.session_fingerprint IS NOT NULL")
            .bind(connection_hash.as_slice())
            .fetch_optional(&mut *tx)
            .await?
        else {
            return Ok(None);
        };
        if timestamp(row.get("session_expires_at"))? <= now {
            return Ok(None);
        }
        let token: [u8; 32] = row
            .get::<Vec<u8>, _>("token_fingerprint")
            .try_into()
            .map_err(|_| invalid())?;
        let bearer = derive_session_bearer(
            self.host_key.session_hmac_key(),
            &token,
            SessionBearerPurpose::ConnectorSession,
        );
        if row.get::<Vec<u8>, _>("session_fingerprint").as_slice() != bearer.fingerprint {
            return Ok(None);
        }
        sqlx::query("UPDATE room_connector_invites SET session_expires_at = MAX(session_expires_at, ?) WHERE invite_id = ?")
            .bind((now + TTL).timestamp_micros())
            .bind(row.get::<String, _>("invite_id"))
            .execute(&mut *tx)
            .await?;
        // The seat must still be in the room and the room still active.
        if authorize_in(&mut tx, &bearer.fingerprint, now)
            .await
            .is_err()
        {
            return Ok(None);
        }
        let resumed = ResumedConnector {
            session_bearer: bearer.bearer,
            room_id: row.get("room_id"),
            room_uid: parse_uuid(row.get("room_uid"))?,
            participant_id: row.get("participant_id"),
            invitation_identity: row
                .get::<Vec<u8>, _>("invitation_identity")
                .try_into()
                .map_err(|_| invalid())?,
            display_name: row.get("display_name"),
        };
        tx.commit().await?;
        Ok(Some(resumed))
    }
}

fn invalid() -> PersistenceError {
    crate::connector_session::rejected("invalid_state", "Stored connector connection is invalid.")
}

pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 100) {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query(SEAT_DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '100' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
