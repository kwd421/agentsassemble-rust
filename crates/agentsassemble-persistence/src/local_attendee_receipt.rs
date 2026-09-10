//! A pre-dispatch receipt prevents process loss from making an old local launch look unused.
use agentsassemble_domain::{LocalAttendeePhase, LocalAttendeeStatus};
use uuid::Uuid;

use crate::{PersistenceError, SqliteStore};

impl SqliteStore {
    /// Reserves both local request and invitation identity before any remote admission.
    /// Stores no bearer, client secret, credentials, executable, workspace or creation payload.
    ///
    /// # Errors
    /// Rejects a previously reserved identity, invalid receipt or unavailable storage.
    pub async fn reserve_local_attendee(
        &self,
        receipt: &LocalAttendeeStatus,
        invitation_identity: &[u8; 32],
    ) -> Result<(), PersistenceError> {
        validate(receipt, receipt.request_id)?;
        let mut transaction = self.pool.begin().await?;
        let invitation = hex::encode(invitation_identity);
        for (key, value) in [
            (
                request_key(receipt.request_id),
                serde_json::to_string(receipt)?,
            ),
            (
                format!("local_attendee_invitation_v1:{invitation}"),
                receipt.request_id.to_string(),
            ),
        ] {
            let result = sqlx::query(
                "INSERT INTO runtime_metadata (key,value) VALUES (?,?) ON CONFLICT(key) DO NOTHING",
            )
            .bind(key)
            .bind(value)
            .execute(&mut *transaction)
            .await?;
            if result.rows_affected() != 1 {
                return Err(PersistenceError::CommandConflict);
            }
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Reads the immutable process-loss receipt. A live owner supplies its own current status.
    ///
    /// # Errors
    /// Rejects corrupt receipts and storage failures; neither is a missing request.
    pub async fn local_attendee_receipt(
        &self,
        request_id: Uuid,
    ) -> Result<Option<LocalAttendeeStatus>, PersistenceError> {
        let stored: Option<String> =
            sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key = ?")
                .bind(request_key(request_id))
                .fetch_optional(&self.pool)
                .await?;
        let Some(stored) = stored else {
            return Ok(None);
        };
        let receipt = serde_json::from_str(&stored)?;
        validate(&receipt, request_id)?;
        Ok(Some(receipt))
    }
}

fn request_key(id: Uuid) -> String {
    format!("local_attendee_v1:{id}")
}

fn validate(receipt: &LocalAttendeeStatus, id: Uuid) -> Result<(), PersistenceError> {
    if receipt.request_id != id
        || id.is_nil()
        || receipt.room_uid.is_nil()
        || agentsassemble_domain::validate_room_id(&receipt.room_id)
            .ok()
            .as_ref()
            != Some(&receipt.room_id)
        || receipt.participant_id.is_some()
        || receipt.phase != LocalAttendeePhase::CleanupUnconfirmed
        || receipt.error_code.as_deref() != Some("local_attendee_process_restarted")
    {
        return Err(PersistenceError::CommandRejected {
            code: "invalid_local_attendee_receipt".into(),
            message: "Local attendee receipt is invalid.".to_owned(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reopen_retains_identity_conflicts_and_corruption_is_not_missing()
    -> Result<(), Box<dyn std::error::Error>> {
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path).await?;
        let receipt = LocalAttendeeStatus {
            request_id: Uuid::new_v4(),
            room_id: "general".to_owned(),
            room_uid: Uuid::new_v4(),
            participant_id: None,
            phase: LocalAttendeePhase::CleanupUnconfirmed,
            error_code: Some("local_attendee_process_restarted".to_owned()),
        };
        store.reserve_local_attendee(&receipt, &[7; 32]).await?;
        drop(store);
        let store = SqliteStore::open_path(&path).await?;
        assert_eq!(
            store.local_attendee_receipt(receipt.request_id).await?,
            Some(receipt.clone())
        );
        assert!(matches!(
            store.reserve_local_attendee(&receipt, &[8; 32]).await,
            Err(PersistenceError::CommandConflict)
        ));
        let other = LocalAttendeeStatus {
            request_id: Uuid::new_v4(),
            ..receipt.clone()
        };
        assert!(matches!(
            store.reserve_local_attendee(&other, &[7; 32]).await,
            Err(PersistenceError::CommandConflict)
        ));
        assert!(
            store
                .local_attendee_receipt(other.request_id)
                .await?
                .is_none()
        );
        store.reserve_local_attendee(&other, &[8; 32]).await?;
        sqlx::query("UPDATE runtime_metadata SET value = '{' WHERE key = ?")
            .bind(request_key(receipt.request_id))
            .execute(&store.pool)
            .await?;
        assert!(
            store
                .local_attendee_receipt(receipt.request_id)
                .await
                .is_err()
        );
        Ok(())
    }
}
