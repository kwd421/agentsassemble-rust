//! Durable server-owner custody, independent of room existence and entry-grant TTL.
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    PersistenceError, SqliteStore,
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

pub const OWNER_SESSION_PREFIX: &str = "aaos1.";
pub const OWNER_LEASE_SECONDS: i64 = 60;
pub(crate) const DDL: &str = "CREATE TABLE central_owner_sessions (
    fingerprint BLOB PRIMARY KEY CHECK(length(fingerprint)=32),
    connection_id TEXT NOT NULL UNIQUE, server_id TEXT NOT NULL,
    person_id TEXT NOT NULL, device_id TEXT NOT NULL,
    browser_fingerprint BLOB NOT NULL CHECK(length(browser_fingerprint)=32),
    origin TEXT NOT NULL, generation INTEGER NOT NULL,
    session_expires_at INTEGER NOT NULL, expires_at INTEGER NOT NULL,
    renew_at INTEGER NOT NULL, revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))) STRICT";

/// Immutable provenance returned only by the configured central host-signed endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnerConnectionBinding {
    pub connection_id: String,
    pub server_id: String,
    pub person_id: String,
    pub device_id: String,
    pub browser_fingerprint: [u8; 32],
    pub origin: String,
    pub generation: i64,
    pub session_expires_at: i64,
}

/// A bounded central authorization result, with no wire deserializer.
pub struct OwnerConnectionLease {
    binding: OwnerConnectionBinding,
    expires_at: i64,
    renew_at: i64,
}

impl OwnerConnectionLease {
    /// # Errors
    /// Rejects invalid provenance, stale leases and excessive authorization lifetime.
    pub fn verified(
        binding: OwnerConnectionBinding,
        expires_at: i64,
        renew_at: i64,
    ) -> Result<Self, PersistenceError> {
        let now = Utc::now().timestamp();
        if !binding.connection_id.starts_with("soc_")
            || binding.connection_id.len() != 47
            || uuid::Uuid::parse_str(&binding.server_id).is_err()
            || binding.person_id.is_empty()
            || binding.device_id.is_empty()
            || binding.generation < 1
            || expires_at <= now
            || expires_at > now + OWNER_LEASE_SECONDS
            || binding.session_expires_at < expires_at
            || DateTime::from_timestamp(binding.session_expires_at, 0).is_none()
            || renew_at <= now
            || renew_at > expires_at
        {
            return Err(invalid());
        }
        crate::operator_pairing::require_origin(&binding.origin)?;
        Ok(Self {
            binding,
            expires_at,
            renew_at,
        })
    }
}

#[derive(Clone)]
pub struct OwnerSessionAuthorization {
    fingerprint: [u8; 32],
    binding: OwnerConnectionBinding,
    expires_at: i64,
    renew_at: i64,
}

impl OwnerSessionAuthorization {
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    #[must_use]
    pub const fn binding(&self) -> &OwnerConnectionBinding {
        &self.binding
    }
    #[must_use]
    pub const fn expires_at(&self) -> i64 {
        self.expires_at
    }
    #[must_use]
    pub const fn renew_at(&self) -> i64 {
        self.renew_at
    }

    pub(crate) async fn revalidate(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
    ) -> Result<Self, PersistenceError> {
        let current = resolve(
            tx,
            &self.fingerprint,
            &self.binding.browser_fingerprint,
            &self.binding.origin,
        )
        .await?;
        if current.binding != self.binding {
            return Err(invalid());
        }
        Ok(current)
    }
}

pub struct OwnerSessionRedemption {
    pub session_bearer: String,
    pub authorization: OwnerSessionAuthorization,
}

impl SqliteStore {
    /// Commits or exactly replays the same centrally exchanged owner connection.
    /// # Errors
    /// Rejects a foreign server/device, stale generation, revoked replay or capacity.
    pub async fn create_owner_session(
        &self,
        lease: &OwnerConnectionLease,
    ) -> Result<OwnerSessionRedemption, PersistenceError> {
        let seed = Sha256::digest(lease.binding.connection_id.as_bytes()).into();
        let issued = derive_session_bearer(
            self.host_key.session_hmac_key(),
            &seed,
            SessionBearerPurpose::ServerOwner,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_binding(&mut tx, &lease.binding).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM central_owner_sessions WHERE fingerprint = ?)",
        )
        .bind(issued.fingerprint.as_slice())
        .fetch_one(&mut *tx)
        .await?;
        if exists {
            let current = resolve(
                &mut tx,
                &issued.fingerprint,
                &lease.binding.browser_fingerprint,
                &lease.binding.origin,
            )
            .await?;
            if current.binding != lease.binding {
                return Err(invalid());
            }
        } else {
            sqlx::query("DELETE FROM central_owner_sessions WHERE expires_at <= ?")
                .bind(Utc::now().timestamp())
                .execute(&mut *tx)
                .await?;
            let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM central_owner_sessions")
                .fetch_one(&mut *tx)
                .await?;
            if count >= 128 {
                return Err(PersistenceError::CommandRejected {
                    code: "pairing_capacity".into(),
                    message: "Server owner session capacity is unavailable.".into(),
                });
            }
            sqlx::query("INSERT INTO central_owner_sessions (fingerprint, connection_id, server_id, person_id, device_id, browser_fingerprint, origin, generation, session_expires_at, expires_at, renew_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(issued.fingerprint.as_slice()).bind(&lease.binding.connection_id).bind(&lease.binding.server_id)
                .bind(&lease.binding.person_id).bind(&lease.binding.device_id).bind(lease.binding.browser_fingerprint.as_slice())
                .bind(&lease.binding.origin).bind(lease.binding.generation).bind(lease.binding.session_expires_at)
                .bind(lease.expires_at).bind(lease.renew_at).execute(&mut *tx).await?;
        }
        let authorization = resolve(
            &mut tx,
            &issued.fingerprint,
            &lease.binding.browser_fingerprint,
            &lease.binding.origin,
        )
        .await?;
        tx.commit().await?;
        Ok(OwnerSessionRedemption {
            session_bearer: issued.bearer,
            authorization,
        })
    }

    /// # Errors
    /// Rejects expired/revoked custody, another device/origin or changed endpoint.
    pub async fn authorize_owner_session(
        &self,
        fingerprint: &[u8; 32],
        device: &[u8; 32],
        origin: &str,
    ) -> Result<OwnerSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let authorization = resolve(&mut tx, fingerprint, device, origin).await?;
        tx.commit().await?;
        Ok(authorization)
    }

    /// Updates the parent's lease atomically; room credentials keep their identity.
    /// # Errors
    /// Rejects expired/revoked parents and any change to immutable provenance.
    pub async fn renew_owner_session(
        &self,
        expected: &OwnerSessionAuthorization,
        lease: &OwnerConnectionLease,
    ) -> Result<OwnerSessionAuthorization, PersistenceError> {
        if lease.binding != expected.binding {
            return Err(invalid());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        expected.revalidate(&mut tx).await?;
        sqlx::query(
            "UPDATE central_owner_sessions SET expires_at = MAX(expires_at, ?), renew_at = CASE WHEN expires_at <= ? THEN ? ELSE renew_at END WHERE fingerprint = ?",
        )
        .bind(lease.expires_at)
        .bind(lease.expires_at)
        .bind(lease.renew_at)
        .bind(expected.fingerprint.as_slice())
        .execute(&mut *tx)
        .await?;
        let current = resolve(
            &mut tx,
            &expected.fingerprint,
            &expected.binding.browser_fingerprint,
            &expected.binding.origin,
        )
        .await?;
        tx.commit().await?;
        Ok(current)
    }

    /// # Errors
    /// Propagates storage failure; a failed write never reports revoked authority.
    pub async fn revoke_owner_session(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<(), PersistenceError> {
        sqlx::query("UPDATE central_owner_sessions SET revoked = 1 WHERE fingerprint = ?")
            .bind(fingerprint.as_slice())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

pub(crate) async fn resolve(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    device: &[u8; 32],
    origin: &str,
) -> Result<OwnerSessionAuthorization, PersistenceError> {
    let row = sqlx::query("SELECT * FROM central_owner_sessions WHERE fingerprint = ?")
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(invalid)?;
    let binding = OwnerConnectionBinding {
        connection_id: row.try_get("connection_id")?,
        server_id: row.try_get("server_id")?,
        person_id: row.try_get("person_id")?,
        device_id: row.try_get("device_id")?,
        browser_fingerprint: row
            .try_get::<Vec<u8>, _>("browser_fingerprint")?
            .try_into()
            .map_err(|_| invalid())?,
        origin: row.try_get("origin")?,
        generation: row.try_get("generation")?,
        session_expires_at: row.try_get("session_expires_at")?,
    };
    let expires_at: i64 = row.try_get("expires_at")?;
    if row.try_get::<bool, _>("revoked")?
        || binding.browser_fingerprint != *device
        || binding.origin != origin
        || expires_at <= Utc::now().timestamp()
        || binding.session_expires_at < expires_at
    {
        return Err(invalid());
    }
    validate_binding(tx, &binding).await?;
    Ok(OwnerSessionAuthorization {
        fingerprint: *fingerprint,
        binding,
        expires_at,
        renew_at: row.try_get("renew_at")?,
    })
}

async fn validate_binding(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &OwnerConnectionBinding,
) -> Result<(), PersistenceError> {
    let bootstrap = crate::bootstrap::require_complete_bootstrap_in_transaction(tx).await?;
    let generation: Option<String> = sqlx::query_scalar(
        "SELECT value FROM runtime_metadata WHERE key = 'central_endpoint_generation'",
    )
    .fetch_optional(&mut **tx)
    .await?;
    if bootstrap.server_id != binding.server_id
        || generation.as_deref() != Some(binding.generation.to_string().as_str())
    {
        return Err(invalid());
    }
    Ok(())
}

pub(crate) fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "central_owner_session_invalid".into(),
        message: "Server owner access has expired or been revoked.".into(),
    }
}
