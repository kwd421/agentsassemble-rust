//! Durable person-local removal fence. No central delivery, inventory or ACK.
use chrono::{DateTime, Utc};
use sqlx::{Row, Sqlite, SqlitePool, Transaction};

use crate::{PersistenceError, SecureSessionBinding, SqliteStore};

pub(crate) const DDL: &str = "CREATE TABLE central_member_removals (
    issuer TEXT NOT NULL, person_id TEXT NOT NULL,
    user_id TEXT REFERENCES user_profiles(user_id) ON DELETE RESTRICT,
    participant_id TEXT, request_id TEXT NOT NULL, phase TEXT NOT NULL,
    cursor TEXT NOT NULL DEFAULT '', schema_revision INTEGER NOT NULL CHECK(schema_revision=87),
    CHECK((user_id IS NULL)=(participant_id IS NULL)), PRIMARY KEY(issuer,person_id)
) STRICT";

/// Constructed only from an issuer-pinned deletion-purpose secure redemption.
/// No deserialization, debug output, room authority, proof or receipt borrowing.
pub struct MemberRemovalPrincipal {
    issuer: String,
    person_id: String,
    request_id: String,
    epoch: String,
    expires_at: DateTime<Utc>,
    secure: SecureSessionBinding,
}

impl MemberRemovalPrincipal {
    /// # Errors
    /// Rejects malformed or expired verified transport/identity custody.
    pub fn verified(
        issuer: String,
        person_id: String,
        request_id: String,
        epoch: String,
        expires_at: DateTime<Utc>,
        secure: SecureSessionBinding,
    ) -> Result<Self, PersistenceError> {
        let now = Utc::now();
        let origin = url::Url::parse(&issuer).map_err(|_| invalid())?;
        if origin.origin().ascii_serialization() != issuer
            || (origin.scheme() != "https"
                && !(origin.scheme() == "http"
                    && origin.host_str().is_some_and(|h| {
                        h == "localhost"
                            || h.parse::<std::net::IpAddr>()
                                .is_ok_and(|ip| ip.is_loopback())
                    })))
            || person_id.is_empty()
            || person_id.len() > 256
            || person_id.chars().any(char::is_control)
            || !(32..=128).contains(&request_id.len())
            || !request_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._:-".contains(&b))
            || epoch.is_empty()
            || epoch.len() > 128
            || secure.channel_id.len() != 43
            || expires_at <= now
            || expires_at > now + chrono::Duration::seconds(300)
        {
            return Err(invalid());
        }
        Ok(Self {
            issuer,
            person_id,
            request_id,
            epoch,
            expires_at,
            secure,
        })
    }
}

/// Existing committed work, not authority to select or admit an identity.
#[derive(Clone)]
pub struct MemberRemovalKey {
    pub(crate) issuer: String,
    pub(crate) person_id: String,
}

impl MemberRemovalKey {
    #[must_use]
    pub fn issuer(&self) -> &str {
        &self.issuer
    }
    #[must_use]
    pub fn person_id(&self) -> &str {
        &self.person_id
    }
}

impl SqliteStore {
    /// Commits the person fence and initial work together. A dropped request cannot undo it.
    /// # Errors
    /// Rejects stale epoch, expired proof-bound authority and a different secure channel.
    pub async fn begin_member_account_removal(
        &self,
        principal: &MemberRemovalPrincipal,
        presented: &SecureSessionBinding,
    ) -> Result<MemberRemovalKey, PersistenceError> {
        SecureSessionBinding::require_match(Some(&principal.secure), Some(presented))?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_schema(&mut tx).await?;
        let epoch: Option<String> = sqlx::query_scalar(
            "SELECT value FROM runtime_metadata WHERE key='central_registration_epoch'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if epoch.as_deref() != Some(&principal.epoch) || principal.expires_at <= Utc::now() {
            return Err(invalid());
        }
        let binding = sqlx::query("SELECT b.user_id,p.participant_id FROM central_identity_bindings b JOIN user_profiles p USING(user_id) WHERE b.issuer=? AND b.person_id=?")
            .bind(&principal.issuer).bind(&principal.person_id).fetch_optional(&mut *tx).await?;
        let user: Option<String> = binding.as_ref().map(|r| r.try_get("user_id")).transpose()?;
        let actor: Option<String> = binding
            .as_ref()
            .map(|r| r.try_get("participant_id"))
            .transpose()?;
        sqlx::query("INSERT INTO central_member_removals(issuer,person_id,user_id,participant_id,request_id,phase,schema_revision) VALUES (?,?,?,?,?,'owner_sessions',87) ON CONFLICT(issuer,person_id) DO NOTHING")
            .bind(&principal.issuer).bind(&principal.person_id).bind(user).bind(actor)
            .bind(&principal.request_id).execute(&mut *tx).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(MemberRemovalKey {
            issuer: principal.issuer.clone(),
            person_id: principal.person_id.clone(),
        })
    }
}

pub(crate) async fn require_schema(
    tx: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut **tx)
            .await?;
    if version != "87" {
        return Err(PersistenceError::InvalidSchemaVersion(version));
    }
    Ok(())
}

pub(crate) async fn person_is_live(
    tx: &mut Transaction<'_, Sqlite>,
    issuer: &str,
    person: &str,
) -> Result<bool, PersistenceError> {
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut **tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|v| (70..=86).contains(&v)) {
        return Ok(true);
    }
    require_schema(tx).await?;
    let removed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM central_member_removals WHERE issuer=? AND person_id=?)",
    )
    .bind(issuer)
    .bind(person)
    .fetch_one(&mut **tx)
    .await?;
    Ok(!removed)
}

pub(crate) async fn require_live_actor(
    tx: &mut Transaction<'_, Sqlite>,
    participant: &str,
) -> Result<(), PersistenceError> {
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut **tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|v| (70..=86).contains(&v)) {
        return Ok(());
    }
    require_schema(tx).await?;
    let removed: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM central_member_removals r JOIN central_identity_bindings b ON b.issuer=r.issuer AND b.person_id=r.person_id JOIN user_profiles p ON p.user_id=b.user_id WHERE p.participant_id=?)")
        .bind(participant).fetch_one(&mut **tx).await?;
    if removed {
        return Err(crate::authority::session_revoked());
    }
    Ok(())
}

// Host owner sessions are issued only by the configured pinned central issuer.
// They predate issuer storage; use their actual person custody, never the shared local actor.
pub(crate) async fn require_live_owner_person(
    tx: &mut Transaction<'_, Sqlite>,
    person: &str,
) -> Result<(), PersistenceError> {
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut **tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|v| (70..=86).contains(&v)) {
        return Ok(());
    }
    require_schema(tx).await?;
    let removed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM central_member_removals WHERE person_id=?)",
    )
    .bind(person)
    .fetch_one(&mut **tx)
    .await?;
    if removed {
        return Err(crate::host_owner_session::invalid());
    }
    Ok(())
}

pub(crate) async fn upgrade(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version == "86" {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value='87' WHERE key='schema_version'")
            .execute(&mut *tx)
            .await?;
    } else if version != "87" {
        return Err(PersistenceError::InvalidSchemaVersion(version));
    }
    tx.commit().await?;
    Ok(())
}

fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "account_removal_authority_invalid".into(),
        message: "A current proof-bound secure self-removal admission is required.".into(),
    }
}

#[cfg(test)]
#[path = "central_member_removal_tests.rs"]
mod tests;
