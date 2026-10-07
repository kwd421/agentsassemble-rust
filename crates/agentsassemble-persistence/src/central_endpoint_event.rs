//! One durable pending endpoint event. Delivery never reserves a new generation.
use super::{invalid_generation, reserve_generation};
use crate::{PersistenceError, SqliteStore};
use serde::{Deserialize, Serialize};

const PENDING: &str = "central_endpoint_pending_event";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CentralEndpointEvent {
    pub registration_epoch: String,
    pub origin: String,
    pub generation: i64,
    pub created_at_ms: i64,
    pub failed_attempts: u8,
}
impl CentralEndpointEvent {
    #[must_use]
    pub fn next_attempt_at_ms(&self) -> Option<i64> {
        [0, 2000, 8000]
            .get(usize::from(self.failed_attempts))
            .map(|delay| self.created_at_ms.saturating_add(*delay))
    }
    #[must_use]
    pub fn online(&self) -> bool {
        !self.origin.is_empty()
    }
}

impl SqliteStore {
    /// Reserves a fixed generation and replaces pending delivery at actual event creation.
    /// # Errors
    /// Rejects stale registration, exhausted generation or failed durable writes.
    pub async fn reserve_central_endpoint_event(
        &self,
        epoch: &str,
        origin: &str,
        now_ms: i64,
    ) -> Result<CentralEndpointEvent, PersistenceError> {
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        let current: Option<String> = sqlx::query_scalar(
            "SELECT value FROM runtime_metadata WHERE key='central_registration_epoch'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if epoch.is_empty() || current.as_deref() != Some(epoch) {
            return Err(invalid_generation());
        }
        let event = CentralEndpointEvent {
            registration_epoch: epoch.into(),
            origin: origin.into(),
            generation: reserve_generation(&mut tx).await?,
            created_at_ms: now_ms,
            failed_attempts: 0,
        };
        sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value")
            .bind(PENDING).bind(serde_json::to_string(&event)?).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(event)
    }

    /// # Errors
    /// Surfaces malformed or unreadable pending publication state.
    pub async fn pending_central_endpoint_event(
        &self,
    ) -> Result<Option<CentralEndpointEvent>, PersistenceError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key=?")
                .bind(PENDING)
                .fetch_optional(&self.pool)
                .await?;
        value
            .map(|value| serde_json::from_str(&value).map_err(PersistenceError::from))
            .transpose()
    }

    /// ACK/failure updates only the exact captured pending event and attempt.
    /// Superseded responses never clear, park or change failure counts of newer events.
    /// # Errors
    /// Surfaces persistence failures without reporting a successful delivery.
    pub async fn finish_central_endpoint_event(
        &self,
        event: &CentralEndpointEvent,
        succeeded: bool,
    ) -> Result<bool, PersistenceError> {
        let captured = serde_json::to_string(event)?;
        let result = if succeeded {
            sqlx::query("DELETE FROM runtime_metadata WHERE key=? AND value=?")
                .bind(PENDING)
                .bind(captured)
                .execute(&self.pool)
                .await?
        } else {
            let mut failed = event.clone();
            failed.failed_attempts = failed.failed_attempts.saturating_add(1).min(3);
            sqlx::query("UPDATE runtime_metadata SET value=? WHERE key=? AND value=?")
                .bind(serde_json::to_string(&failed)?)
                .bind(PENDING)
                .bind(captured)
                .execute(&self.pool)
                .await?
        };
        Ok(result.rows_affected() == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn endpoint_retries_park_without_generation_changes_and_late_ack_cannot_clear_replacement()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let path = dir.path().join("endpoint.sqlite3");
        let store = SqliteStore::open_path(&path).await?;
        store.set_registration_epoch(Some("epoch-one")).await?;
        let first = store
            .reserve_central_endpoint_event("epoch-one", "https://one.test", 10000)
            .await?;
        let mut retry = first.clone();
        for due in [10000, 12000, 18000] {
            assert_eq!(retry.next_attempt_at_ms(), Some(due));
            assert_eq!(retry.generation, first.generation);
            assert!(store.finish_central_endpoint_event(&retry, false).await?);
            retry = store
                .pending_central_endpoint_event()
                .await?
                .ok_or("pending")?;
        }
        assert_eq!(retry.next_attempt_at_ms(), None);
        store.close().await?;
        let reopened = SqliteStore::open_path(&path).await?;
        assert_eq!(
            reopened.pending_central_endpoint_event().await?,
            Some(retry.clone())
        );
        let replacement = reopened
            .reserve_central_endpoint_event("epoch-one", "https://two.test", 20000)
            .await?;
        assert!(replacement.generation > first.generation);
        assert!(!reopened.finish_central_endpoint_event(&retry, true).await?);
        assert!(
            !reopened
                .finish_central_endpoint_event(&retry, false)
                .await?
        );
        assert_eq!(
            reopened.pending_central_endpoint_event().await?,
            Some(replacement.clone())
        );
        assert!(
            reopened
                .finish_central_endpoint_event(&replacement, true)
                .await?
        );
        assert_eq!(
            reopened.current_central_endpoint_generation().await?,
            replacement.generation
        );
        reopened.set_registration_epoch(Some("epoch-two")).await?;
        assert!(
            reopened
                .reserve_central_endpoint_event("epoch-one", "", 30000)
                .await
                .is_err()
        );
        let offline = reopened
            .reserve_central_endpoint_event("epoch-two", "", 30000)
            .await?;
        assert!(!offline.online());
        assert!(offline.generation > replacement.generation);
        Ok(())
    }
}
