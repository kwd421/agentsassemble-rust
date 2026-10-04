//! Host-owned admission and connection state. Central identity is checked only on entry.
use chrono::Utc;
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, SqliteStore,
    session_bearer::{SessionBearerPurpose, derive_session_bearer},
};

pub const OWNER_SESSION_PREFIX: &str = "aaos1.";
pub(crate) const DDL: &str = "CREATE TABLE host_owner_sessions (
    fingerprint BLOB PRIMARY KEY CHECK(length(fingerprint)=32),
    session_id TEXT NOT NULL UNIQUE, entry_fingerprint BLOB NOT NULL UNIQUE CHECK(length(entry_fingerprint)=32),
    server_id TEXT NOT NULL, person_id TEXT NOT NULL, device_id TEXT NOT NULL,
    browser_fingerprint BLOB NOT NULL CHECK(length(browser_fingerprint)=32),
    origin TEXT NOT NULL, generation INTEGER NOT NULL, admission_expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL, last_connected_at INTEGER NOT NULL,
    device_name TEXT NOT NULL, browser TEXT NOT NULL, os TEXT NOT NULL,
    connected INTEGER NOT NULL DEFAULT 1 CHECK(connected IN (0,1)),
    revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0,1))) STRICT";

/// Descriptive metadata is bounded text, never authentication evidence.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct OwnerDeviceDescription {
    pub device_name: String,
    pub browser: String,
    pub os: String,
}

impl OwnerDeviceDescription {
    /// # Errors
    /// Rejects control characters and oversized device descriptions.
    pub fn verified(
        device_name: String,
        browser: String,
        os: String,
    ) -> Result<Self, PersistenceError> {
        if [&device_name, &browser, &os]
            .iter()
            .any(|value| value.len() > 160 || value.chars().any(char::is_control))
        {
            return Err(invalid());
        }
        Ok(Self {
            device_name,
            browser,
            os,
        })
    }
}

/// Immutable provenance from a host-signed central entry redemption.
#[derive(Clone, PartialEq, Eq)]
pub struct OwnerAdmissionBinding {
    pub entry_fingerprint: [u8; 32],
    pub server_id: String,
    pub person_id: String,
    pub device_id: String,
    pub browser_fingerprint: [u8; 32],
    pub origin: String,
    pub generation: i64,
}

/// An entry deadline permits initial connection/retry; it never limits connected authority.
pub struct OwnerAdmission {
    pub(crate) binding: OwnerAdmissionBinding,
    pub(crate) expires_at: i64,
}

impl OwnerAdmission {
    /// # Errors
    /// Rejects malformed identity, origin, generation or an expired/excessive entry window.
    pub fn verified(
        binding: OwnerAdmissionBinding,
        expires_at: i64,
    ) -> Result<Self, PersistenceError> {
        let now = Utc::now().timestamp();
        if Uuid::parse_str(&binding.server_id).is_err()
            || binding.generation < 1
            || [&binding.person_id, &binding.device_id]
                .iter()
                .any(|value| value.is_empty() || value.len() > 200)
            || expires_at <= now
            || expires_at > now + 300
        {
            return Err(invalid());
        }
        crate::operator_pairing::require_origin(&binding.origin)?;
        Ok(Self {
            binding,
            expires_at,
        })
    }
}

#[derive(Clone)]
pub struct OwnerSessionAuthorization {
    fingerprint: [u8; 32],
    session_id: Uuid,
    binding: OwnerAdmissionBinding,
    admission_expires_at: i64,
}

impl OwnerSessionAuthorization {
    #[must_use]
    pub const fn fingerprint(&self) -> &[u8; 32] {
        &self.fingerprint
    }
    #[must_use]
    pub const fn session_id(&self) -> Uuid {
        self.session_id
    }
    #[must_use]
    pub const fn binding(&self) -> &OwnerAdmissionBinding {
        &self.binding
    }
    #[must_use]
    pub const fn admission_expires_at(&self) -> i64 {
        self.admission_expires_at
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
        if current.binding != self.binding || current.session_id != self.session_id {
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
    /// Issues one host session or exactly replays a still-connected same-browser admission.
    /// # Errors
    /// Rejects foreign custody, stale generation, disconnected/revoked replay or capacity.
    pub async fn create_owner_session(
        &self,
        admission: &OwnerAdmission,
        description: &OwnerDeviceDescription,
    ) -> Result<OwnerSessionRedemption, PersistenceError> {
        let binding = &admission.binding;
        let issued = derive_session_bearer(
            self.host_key.session_hmac_key(),
            &binding.entry_fingerprint,
            SessionBearerPurpose::ServerOwner,
        );
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        validate_admission(&mut tx, binding).await?;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM host_owner_sessions WHERE entry_fingerprint = ?)",
        )
        .bind(binding.entry_fingerprint.as_slice())
        .fetch_one(&mut *tx)
        .await?;
        if !exists {
            // Retain consumed entries until their grant expires, and retain an issuer
            // while a separately paired device still depends on its revocation state.
            sqlx::query("DELETE FROM host_owner_sessions WHERE connected = 0 AND admission_expires_at <= ? AND NOT EXISTS (SELECT 1 FROM operator_pairings WHERE host_owner_session_fingerprint = host_owner_sessions.fingerprint AND central_owner = 0 AND revoked = 0 AND COALESCE(session_expires_at, expires_at) > ?)")
                .bind(Utc::now().timestamp()).bind(Utc::now().timestamp_micros()).execute(&mut *tx).await?;
            let count: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM host_owner_sessions WHERE connected = 1 AND revoked = 0",
            )
            .fetch_one(&mut *tx)
            .await?;
            if count >= 128 {
                return Err(PersistenceError::CommandRejected {
                    code: "pairing_capacity".into(),
                    message: "Server owner session capacity is unavailable.".into(),
                });
            }
            let now = Utc::now().timestamp();
            sqlx::query("INSERT INTO host_owner_sessions (fingerprint, session_id, entry_fingerprint, server_id, person_id, device_id, browser_fingerprint, origin, generation, admission_expires_at, created_at, last_connected_at, device_name, browser, os) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(issued.fingerprint.as_slice()).bind(Uuid::new_v4().to_string()).bind(binding.entry_fingerprint.as_slice())
                .bind(&binding.server_id).bind(&binding.person_id).bind(&binding.device_id).bind(binding.browser_fingerprint.as_slice())
                .bind(&binding.origin).bind(binding.generation).bind(admission.expires_at).bind(now).bind(now)
                .bind(&description.device_name).bind(&description.browser).bind(&description.os).execute(&mut *tx).await?;
        }
        let authorization = resolve(
            &mut tx,
            &issued.fingerprint,
            &binding.browser_fingerprint,
            &binding.origin,
        )
        .await?;
        if authorization.binding != *binding
            || authorization.admission_expires_at != admission.expires_at
        {
            return Err(invalid());
        }
        tx.commit().await?;
        Ok(OwnerSessionRedemption {
            session_bearer: issued.bearer,
            authorization,
        })
    }

    /// # Errors
    /// Rejects disconnected/revoked custody or a different server/browser/origin.
    pub async fn authorize_owner_session(
        &self,
        fingerprint: &[u8; 32],
        device: &[u8; 32],
        origin: &str,
    ) -> Result<OwnerSessionAuthorization, PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let current = resolve(&mut tx, fingerprint, device, origin).await?;
        tx.commit().await?;
        Ok(current)
    }

    /// Runtime construction ends previous connection authority without revoking independent pairings.
    /// # Errors
    /// Propagates storage failure; a restarted runtime never adopts previous live connections.
    pub async fn disconnect_all_owner_sessions(&self) -> Result<(), PersistenceError> {
        sqlx::query("UPDATE host_owner_sessions SET connected = 0 WHERE connected = 1")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// # Errors
    /// Propagates failure to persist the end of this exact admission.
    pub async fn disconnect_owner_session(
        &self,
        fingerprint: &[u8; 32],
    ) -> Result<(), PersistenceError> {
        sqlx::query("UPDATE host_owner_sessions SET connected = 0 WHERE fingerprint = ?")
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
    let row = sqlx::query("SELECT * FROM host_owner_sessions WHERE fingerprint = ?")
        .bind(fingerprint.as_slice())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(invalid)?;
    let binding = OwnerAdmissionBinding {
        entry_fingerprint: fingerprint_column(&row, "entry_fingerprint")?,
        server_id: row.try_get("server_id")?,
        person_id: row.try_get("person_id")?,
        device_id: row.try_get("device_id")?,
        browser_fingerprint: fingerprint_column(&row, "browser_fingerprint")?,
        origin: row.try_get("origin")?,
        generation: row.try_get("generation")?,
    };
    if row.try_get::<bool, _>("revoked")?
        || !row.try_get::<bool, _>("connected")?
        || binding.browser_fingerprint != *device
        || binding.origin != origin
    {
        return Err(invalid());
    }
    // Publication recovery may advance discovery generation while this host-owned
    // workspace stays connected. Only a new admission checks that generation.
    let bootstrap = crate::bootstrap::require_complete_bootstrap_in_transaction(tx).await?;
    if bootstrap.server_id != binding.server_id {
        return Err(invalid());
    }
    Ok(OwnerSessionAuthorization {
        fingerprint: *fingerprint,
        session_id: Uuid::parse_str(&row.try_get::<String, _>("session_id")?)
            .map_err(|_| invalid())?,
        binding,
        admission_expires_at: row.try_get("admission_expires_at")?,
    })
}

/// A delegated device has its own connection lifetime, but cannot outlive issuer revocation.
pub(crate) async fn require_unrevoked_issuer(
    tx: &mut Transaction<'_, Sqlite>,
    fingerprint: &[u8; 32],
    origin: &str,
) -> Result<(), PersistenceError> {
    let live: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM host_owner_sessions WHERE fingerprint = ? AND origin = ? AND revoked = 0)")
        .bind(fingerprint.as_slice()).bind(origin).fetch_one(&mut **tx).await?;
    if !live {
        return Err(invalid());
    }
    Ok(())
}

async fn validate_admission(
    tx: &mut Transaction<'_, Sqlite>,
    binding: &OwnerAdmissionBinding,
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

pub(crate) fn fingerprint_column(
    row: &sqlx::sqlite::SqliteRow,
    name: &str,
) -> Result<[u8; 32], PersistenceError> {
    row.try_get::<Vec<u8>, _>(name)?
        .try_into()
        .map_err(|_| invalid())
}

pub(crate) fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "central_owner_session_invalid".into(),
        message: "Server owner access has disconnected or been revoked.".into(),
    }
}
