//! Host-owned member resume keys: one public key per browser credential.
//! The private half never reaches this host; only the public key, its issuer,
//! person and epoch are stored. Every read is scoped to the live
//! registration epoch, so a rotated incarnation cannot present an old row.
use crate::{MemberAdmission, PersistenceError, SqliteStore};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};

pub(crate) const DDL: &str = "CREATE TABLE IF NOT EXISTS member_resume_keys (
    issuer TEXT NOT NULL,
    person_id TEXT NOT NULL,
    registration_epoch TEXT NOT NULL,
    browser_credential_fingerprint BLOB NOT NULL CHECK(length(browser_credential_fingerprint) = 32),
    resume_public_key BLOB NOT NULL CHECK(length(resume_public_key) = 65),
    issued_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    PRIMARY KEY(issuer, person_id, browser_credential_fingerprint)
) STRICT";

pub(crate) const LOOKUP_INDEX_DDL: &str = "CREATE INDEX IF NOT EXISTS member_resume_keys_lookup_idx ON member_resume_keys(browser_credential_fingerprint, resume_public_key)";

/// At most this many browser credentials may hold a resume key for one
/// person on this host. The oldest row is dropped first.
pub(crate) const RESUME_KEY_LIMIT: i64 = 8;

/// The stored key for one browser credential. The public key is returned
/// because it is the only verification material this host owns.
pub struct MemberResumeKey {
    pub issuer: String,
    pub person_id: String,
    pub registration_epoch: String,
    pub resume_public_key: [u8; 65],
    pub expires_at: DateTime<Utc>,
}

fn invalid() -> PersistenceError {
    crate::account_identity::rejected(
        "member_resume_invalid",
        "The member resume key is not valid for this request.",
    )
}

fn bounded(value: &str, limit: usize) -> bool {
    !value.is_empty() && value.len() <= limit && !value.chars().any(char::is_control)
}

/// One resume key issuance: the host-owned identity, the device's public key
/// and the host-chosen window. The client contributes only the public key.
pub struct MemberResumeKeyIssue<'a> {
    pub issuer: &'a str,
    pub person_id: &'a str,
    pub registration_epoch: &'a str,
    pub browser_credential_fingerprint: &'a [u8; 32],
    pub resume_public_key: &'a [u8; 65],
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl SqliteStore {
    /// Replaces the resume key held by one browser credential for one person.
    /// The host chooses the expiry; the client never supplies one.
    ///
    /// # Errors
    /// Rejects malformed identities, an expiry that is not in the future and
    /// database failures without storing a partial row.
    pub async fn issue_member_resume_key(
        &self,
        issue: MemberResumeKeyIssue<'_>,
    ) -> Result<(), PersistenceError> {
        let MemberResumeKeyIssue {
            issuer,
            person_id,
            registration_epoch,
            browser_credential_fingerprint,
            resume_public_key,
            issued_at,
            expires_at,
        } = issue;
        if !bounded(issuer, 400)
            || !bounded(person_id, 256)
            || !bounded(registration_epoch, 128)
            || expires_at <= issued_at
        {
            return Err(invalid());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        sqlx::query("INSERT INTO member_resume_keys(issuer,person_id,registration_epoch,browser_credential_fingerprint,resume_public_key,issued_at,expires_at) VALUES (?,?,?,?,?,?,?) ON CONFLICT(issuer,person_id,browser_credential_fingerprint) DO UPDATE SET registration_epoch=excluded.registration_epoch, resume_public_key=excluded.resume_public_key, issued_at=excluded.issued_at, expires_at=excluded.expires_at")
            .bind(issuer)
            .bind(person_id)
            .bind(registration_epoch)
            .bind(browser_credential_fingerprint.as_slice())
            .bind(resume_public_key.as_slice())
            .bind(issued_at.timestamp_micros())
            .bind(expires_at.timestamp_micros())
            .execute(&mut *tx)
            .await?;
        // One account cannot accumulate unbounded resume keys: the oldest row
        // for this person is dropped, never a room session or a device row.
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM member_resume_keys WHERE issuer=? AND person_id=?",
        )
        .bind(issuer)
        .bind(person_id)
        .fetch_one(&mut *tx)
        .await?;
        if count > RESUME_KEY_LIMIT {
            sqlx::query("DELETE FROM member_resume_keys WHERE rowid IN (SELECT rowid FROM member_resume_keys WHERE issuer=? AND person_id=? ORDER BY issued_at ASC, rowid ASC LIMIT ?)")
                .bind(issuer)
                .bind(person_id)
                .bind(count - RESUME_KEY_LIMIT)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Reads the live resume key for one browser credential and public key.
    /// A row from another registration epoch or an expired row is not a key.
    ///
    /// # Errors
    /// Propagates database failures without answering.
    pub async fn member_resume_key(
        &self,
        browser_credential_fingerprint: &[u8; 32],
        resume_public_key: &[u8; 65],
    ) -> Result<Option<MemberResumeKey>, PersistenceError> {
        let row = sqlx::query("SELECT issuer, person_id, registration_epoch, resume_public_key, expires_at FROM member_resume_keys WHERE browser_credential_fingerprint=? AND resume_public_key=? AND registration_epoch=(SELECT value FROM runtime_metadata WHERE key='central_registration_epoch')")
            .bind(browser_credential_fingerprint.as_slice())
            .bind(resume_public_key.as_slice())
            .fetch_optional(&self.pool)
            .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let key: Vec<u8> = row.try_get("resume_public_key")?;
        let Ok(key) = <[u8; 65]>::try_from(key.as_slice()) else {
            return Err(invalid());
        };
        Ok(Some(MemberResumeKey {
            issuer: row.try_get("issuer")?,
            person_id: row.try_get("person_id")?,
            registration_epoch: row.try_get("registration_epoch")?,
            resume_public_key: key,
            expires_at: chrono::DateTime::from_timestamp_micros(row.try_get("expires_at")?)
                .ok_or_else(invalid)?,
        }))
    }

    /// The stored projection anchor for one person on one registration epoch.
    /// The direct path never invents a projection id.
    ///
    /// # Errors
    /// Propagates database failures without answering.
    pub async fn member_projection_anchor(
        &self,
        issuer: &str,
        person_id: &str,
        registration_epoch: &str,
    ) -> Result<Option<String>, PersistenceError> {
        let binding: Option<String> = sqlx::query_scalar(
            "SELECT binding_id FROM central_identity_bindings WHERE issuer=? AND person_id=?",
        )
        .bind(issuer)
        .bind(person_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(binding) = binding else {
            return Ok(None);
        };
        Ok(sqlx::query_scalar(
            "SELECT projection_id FROM member_projection_outbox WHERE binding_id=? AND registration_epoch=?",
        )
        .bind(binding)
        .bind(registration_epoch)
        .fetch_optional(&self.pool)
        .await?)
    }

    /// Builds the direct-path admission from the stored key and the canonical
    /// membership projection. The client supplies neither the person nor the
    /// projection: both come from this host's own durable state. The window is
    /// the same admission window every other connect path uses, so a direct
    /// attempt never moves the replay floor of a later central admission.
    ///
    /// # Errors
    /// Propagates database failures without answering.
    pub async fn member_resume_admission(
        &self,
        key: &MemberResumeKey,
        now: DateTime<Utc>,
        window: chrono::Duration,
    ) -> Result<Option<MemberAdmission>, PersistenceError> {
        if key.expires_at <= now {
            return Ok(None);
        }
        // The stored projection anchor keeps `acked_revision` intact: a direct
        // attempt never rewrites this person's central membership projection.
        let Some(projection_id) = self
            .member_projection_anchor(&key.issuer, &key.person_id, &key.registration_epoch)
            .await?
        else {
            return Ok(None);
        };
        Ok(Some(MemberAdmission {
            secure: None,
            projection_id,
            issuer: key.issuer.clone(),
            person_id: key.person_id.clone(),
            display_name: String::new(),
            registration_epoch: key.registration_epoch.clone(),
            challenge_fingerprint: Sha256::digest(key.resume_public_key).into(),
            challenge_expires_at: now + window,
        }))
    }
}

/// Deletes every resume key held by one person in the caller's transaction.
pub(crate) async fn delete_for_person(
    tx: &mut Transaction<'_, Sqlite>,
    issuer: &str,
    person_id: &str,
) -> Result<u64, PersistenceError> {
    let removed = sqlx::query("DELETE FROM member_resume_keys WHERE issuer=? AND person_id=?")
        .bind(issuer)
        .bind(person_id)
        .execute(&mut **tx)
        .await?;
    Ok(removed.rows_affected())
}

/// Deletes every resume key that belongs to another registration epoch.
pub(crate) async fn delete_outside_epoch(
    tx: &mut Transaction<'_, Sqlite>,
    registration_epoch: Option<&str>,
) -> Result<u64, PersistenceError> {
    let removed =
        sqlx::query("DELETE FROM member_resume_keys WHERE registration_epoch != COALESCE(?, '')")
            .bind(registration_epoch)
            .execute(&mut **tx)
            .await?;
    Ok(removed.rows_affected())
}

/// Creates the resume key table for the current schema version.
pub(crate) async fn upgrade(pool: &sqlx::SqlitePool) -> Result<(), PersistenceError> {
    let mut tx = pool.begin().await?;
    let version: String =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = 'schema_version'")
            .fetch_one(&mut *tx)
            .await?;
    if version.parse::<i64>().is_ok_and(|version| version < 97) {
        sqlx::query(DDL).execute(&mut *tx).await?;
        sqlx::query(LOOKUP_INDEX_DDL).execute(&mut *tx).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '97' WHERE key = 'schema_version'")
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
#[path = "member_resume_tests.rs"]
mod tests;
