//! Fixed-address named-tunnel custody bound to the live registration epoch.
//!
//! The stored token authorizes serving this host's fixed
//! `h-<hex>.agentsassemble.uk` origin from whoever holds it, so it lives in
//! the same private installation authority as the host signing key (the
//! database file secured to `0600` by `private_fs`) and is never logged. The
//! hostname is public; the token is redacted from every diagnostic.

use crate::{PersistenceError, SqliteStore};
use serde::{Deserialize, Serialize};

const STORAGE_KEY: &str = "named_tunnel";
const PROVISION_DAY_KEY: &str = "named_tunnel_provision_day";
const PROVISION_COUNT_KEY: &str = "named_tunnel_provision_count";
const MAX_HOSTNAME_BYTES: usize = 128;
const MAX_TOKEN_BYTES: usize = 8192;
const MAX_EPOCH_BYTES: usize = 128;
/// At most this many central provision attempts per UTC day, so a restart
/// storm or a down central cannot hammer provisioning. Restarts still cost
/// zero calls while a stored token for the live epoch exists.
pub(crate) const MAX_PROVISION_ATTEMPTS_PER_DAY: i64 = 5;

/// A stored fixed-address tunnel credential for one registration epoch.
///
/// The token is deliberately excluded from `Debug` output.
#[derive(Clone)]
pub struct StoredNamedTunnel {
    hostname: String,
    tunnel_token: String,
    registration_epoch: String,
}

impl StoredNamedTunnel {
    #[must_use]
    pub fn hostname(&self) -> &str {
        &self.hostname
    }

    #[must_use]
    pub fn origin(&self) -> String {
        format!("https://{}", self.hostname)
    }

    #[must_use]
    pub fn registration_epoch(&self) -> &str {
        &self.registration_epoch
    }

    /// The opaque Cloudflare tunnel token. Callers must never log it.
    #[must_use]
    pub fn tunnel_token(&self) -> &str {
        &self.tunnel_token
    }
}

impl std::fmt::Debug for StoredNamedTunnel {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredNamedTunnel")
            .field("hostname", &self.hostname)
            .field("tunnel_token", &"[REDACTED]")
            .field("registration_epoch", &self.registration_epoch)
            .finish()
    }
}

/// Fixed hostnames are derived from the server identity, never user text:
/// `h-<12 or 16 lowercase hex>.agentsassemble.uk`.
#[must_use]
pub fn is_valid_named_tunnel_hostname(value: &str) -> bool {
    if value.len() > MAX_HOSTNAME_BYTES {
        return false;
    }
    let Some(labels) = value.strip_suffix(".agentsassemble.uk") else {
        return false;
    };
    let Some(hex) = labels.strip_prefix("h-") else {
        return false;
    };
    (hex.len() == 12 || hex.len() == 16)
        && hex
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn invalid(message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "named_tunnel_invalid".into(),
        message: message.to_owned(),
    }
}

fn validate_fields(
    hostname: &str,
    tunnel_token: &str,
    registration_epoch: &str,
) -> Result<(), PersistenceError> {
    if !is_valid_named_tunnel_hostname(hostname) {
        return Err(invalid("named tunnel hostname is invalid"));
    }
    if tunnel_token.is_empty()
        || tunnel_token.len() > MAX_TOKEN_BYTES
        || tunnel_token.bytes().any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(invalid("named tunnel token is invalid"));
    }
    if registration_epoch.is_empty()
        || registration_epoch.len() > MAX_EPOCH_BYTES
        || registration_epoch
            .bytes()
            .any(|byte| byte < 0x20 || byte == 0x7f)
    {
        return Err(invalid("named tunnel registration epoch is invalid"));
    }
    Ok(())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredPayload {
    hostname: String,
    tunnel_token: String,
    registration_epoch: String,
}

/// Minimal projection used only to compare the stored epoch during an epoch
/// change without failing the update on a malformed row.
#[derive(Deserialize)]
pub(crate) struct StoredPayloadForCleanup {
    pub(crate) registration_epoch: String,
}

impl SqliteStore {
    /// Reads the stored fixed-address credential, if it is well-formed.
    /// Malformed rows are treated as absent; the next successful provision
    /// overwrites them. Database failures are still reported.
    /// # Errors
    /// Returns database read failures.
    pub async fn named_tunnel(&self) -> Result<Option<StoredNamedTunnel>, PersistenceError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(STORAGE_KEY)
                .fetch_optional(&self.pool)
                .await?;
        let Some(value) = value else {
            return Ok(None);
        };
        let payload: StoredPayload = match serde_json::from_str(&value) {
            Ok(payload) => payload,
            Err(_) => return Ok(None),
        };
        if validate_fields(
            &payload.hostname,
            &payload.tunnel_token,
            &payload.registration_epoch,
        )
        .is_err()
        {
            return Ok(None);
        }
        Ok(Some(StoredNamedTunnel {
            hostname: payload.hostname,
            tunnel_token: payload.tunnel_token,
            registration_epoch: payload.registration_epoch,
        }))
    }

    /// Stores a freshly provisioned fixed-address credential.
    /// # Errors
    /// Rejects invalid hostnames, tokens or epochs, and database write failures.
    pub async fn save_named_tunnel(
        &self,
        hostname: &str,
        tunnel_token: &str,
        registration_epoch: &str,
    ) -> Result<StoredNamedTunnel, PersistenceError> {
        validate_fields(hostname, tunnel_token, registration_epoch)?;
        let payload = serde_json::to_string(&StoredPayload {
            hostname: hostname.to_owned(),
            tunnel_token: tunnel_token.to_owned(),
            registration_epoch: registration_epoch.to_owned(),
        })?;
        sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(STORAGE_KEY)
            .bind(payload)
            .execute(&self.pool)
            .await?;
        Ok(StoredNamedTunnel {
            hostname: hostname.to_owned(),
            tunnel_token: tunnel_token.to_owned(),
            registration_epoch: registration_epoch.to_owned(),
        })
    }

    /// Deletes any stored fixed-address credential, e.g. after the
    /// registration epoch changes or central refuses the token.
    /// # Errors
    /// Returns database write failures.
    pub async fn clear_named_tunnel(&self) -> Result<(), PersistenceError> {
        sqlx::query("DELETE FROM runtime_metadata WHERE key = ?")
            .bind(STORAGE_KEY)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Whether another central provision attempt is allowed today. Restarts
    /// with a stored token never reach this gate; it only bounds fresh
    /// provision calls to a few per UTC day.
    /// # Errors
    /// Returns database read failures.
    pub async fn named_tunnel_provision_allowed(
        &self,
        now_secs: i64,
    ) -> Result<bool, PersistenceError> {
        let today = now_secs.div_euclid(86_400);
        let day: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(PROVISION_DAY_KEY)
                .fetch_optional(&self.pool)
                .await?;
        let count: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(PROVISION_COUNT_KEY)
                .fetch_optional(&self.pool)
                .await?;
        match (day, count) {
            (Some(day), Some(count)) if day.parse::<i64>().ok() == Some(today) => {
                Ok(count.parse::<i64>().unwrap_or(0) < MAX_PROVISION_ATTEMPTS_PER_DAY)
            }
            _ => Ok(true),
        }
    }

    /// Records one central provision attempt against today's budget. Callers
    /// must only record attempts that actually sent a request (never the
    /// quota-parked or central-disabled skips).
    /// # Errors
    /// Returns database write failures.
    pub async fn record_named_tunnel_provision_attempt(
        &self,
        now_secs: i64,
    ) -> Result<(), PersistenceError> {
        let today = now_secs.div_euclid(86_400);
        let mut tx = self.pool.begin().await?;
        let day: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(PROVISION_DAY_KEY)
                .fetch_optional(&mut *tx)
                .await?;
        let count: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(PROVISION_COUNT_KEY)
                .fetch_optional(&mut *tx)
                .await?;
        let next = if day.as_deref().and_then(|day| day.parse::<i64>().ok()) == Some(today) {
            count
                .as_deref()
                .and_then(|count| count.parse::<i64>().ok())
                .unwrap_or(0)
                .saturating_add(1)
        } else {
            1
        };
        sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(PROVISION_DAY_KEY).bind(today.to_string()).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value")
            .bind(PROVISION_COUNT_KEY).bind(next.to_string()).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostname_shape_accepts_only_derived_fixed_names() {
        assert!(is_valid_named_tunnel_hostname(
            "h-0123456789ab.agentsassemble.uk"
        ));
        assert!(is_valid_named_tunnel_hostname(
            "h-0123456789abcdef.agentsassemble.uk"
        ));
        for bad in [
            "",
            "h-0123456789ab.example.com",
            "H-0123456789AB.agentsassemble.uk",
            "h-0123456789AB.agentsassemble.uk",
            "h-0123456789a.agentsassemble.uk",
            "h-0123456789abc.agentsassemble.uk",
            "h-0123456789abcde.agentsassemble.uk",
            "h-0123456789abcdef0.agentsassemble.uk",
            "phish.agentsassemble.uk",
            "h-evil.trycloudflare.com",
            "h-0123456789ab.agentsassemble.uk.evil.test",
            "h-0123456789ab.agentsassemble.uk/",
            " https://h-0123456789ab.agentsassemble.uk",
        ] {
            assert!(!is_valid_named_tunnel_hostname(bad), "accepted {bad}");
        }
    }

    #[tokio::test]
    async fn roundtrip_keeps_origin_and_redacts_debug() -> Result<(), Box<dyn std::error::Error>> {
        let store = SqliteStore::open("sqlite::memory:").await?;
        assert!(store.named_tunnel().await?.is_none());
        let stored = store
            .save_named_tunnel(
                "h-0123456789ab.agentsassemble.uk",
                "opaque-token-value",
                "epoch-one",
            )
            .await?;
        assert_eq!(stored.origin(), "https://h-0123456789ab.agentsassemble.uk");
        let loaded = store.named_tunnel().await?.ok_or("stored tunnel")?;
        assert_eq!(loaded.hostname(), stored.hostname());
        assert_eq!(loaded.tunnel_token(), "opaque-token-value");
        assert_eq!(loaded.registration_epoch(), "epoch-one");
        let rendered = format!("{stored:?}");
        assert!(rendered.contains("h-0123456789ab.agentsassemble.uk"));
        assert!(!rendered.contains("opaque-token-value"));
        assert!(rendered.contains("[REDACTED]"));
        store.clear_named_tunnel().await?;
        assert!(store.named_tunnel().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn invalid_rows_are_absent_and_rejected_on_save() -> Result<(), Box<dyn std::error::Error>>
    {
        let store = SqliteStore::open("sqlite::memory:").await?;
        for (hostname, token, epoch) in [
            ("phish.agentsassemble.uk", "token", "epoch"),
            ("h-0123456789ab.agentsassemble.uk", "", "epoch"),
            ("h-0123456789ab.agentsassemble.uk", "token", ""),
        ] {
            assert!(
                store
                    .save_named_tunnel(hostname, token, epoch)
                    .await
                    .is_err()
            );
        }
        sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('named_tunnel', '{\"hostname\":\"bad\"}')")
            .execute(&store.pool)
            .await?;
        assert!(store.named_tunnel().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn epoch_change_clears_the_stored_token() -> Result<(), Box<dyn std::error::Error>> {
        let store = SqliteStore::open("sqlite::memory:").await?;
        store.set_registration_epoch(Some("epoch-one")).await?;
        store
            .save_named_tunnel("h-0123456789ab.agentsassemble.uk", "token-one", "epoch-one")
            .await?;
        store.set_registration_epoch(Some("epoch-two")).await?;
        assert!(store.named_tunnel().await?.is_none());
        store
            .save_named_tunnel("h-0123456789ab.agentsassemble.uk", "token-two", "epoch-two")
            .await?;
        // Re-setting the same epoch keeps the token.
        store.set_registration_epoch(Some("epoch-two")).await?;
        assert!(store.named_tunnel().await?.is_some());
        store.set_registration_epoch(None).await?;
        assert!(store.named_tunnel().await?.is_none());
        Ok(())
    }

    #[tokio::test]
    async fn provision_budget_allows_a_few_attempts_per_day()
    -> Result<(), Box<dyn std::error::Error>> {
        let store = SqliteStore::open("sqlite::memory:").await?;
        let day = 2_000 * 86_400;
        for _ in 0..MAX_PROVISION_ATTEMPTS_PER_DAY {
            assert!(store.named_tunnel_provision_allowed(day).await?);
            store.record_named_tunnel_provision_attempt(day).await?;
        }
        assert!(!store.named_tunnel_provision_allowed(day).await?);
        assert!(store.named_tunnel_provision_allowed(day + 86_400).await?);
        Ok(())
    }
}
