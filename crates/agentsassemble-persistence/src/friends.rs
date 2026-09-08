use agentsassemble_domain::{SaveFriend, SavedFriend};
use chrono::Utc;
use sqlx::{Sqlite, Transaction};
use uuid::Uuid;

use crate::{PersistenceError, SqliteStore, bootstrap::require_complete_bootstrap_in_transaction};

impl SqliteStore {
    /// Reads the local operator address book without inferring presence.
    ///
    /// # Errors
    /// Rejects incomplete bootstrap and propagates storage or decoding failures.
    pub async fn saved_friends(&self) -> Result<Vec<SavedFriend>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        require_complete_bootstrap_in_transaction(&mut transaction).await?;
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT friend_json FROM saved_friends WHERE friend_json IS NOT NULL ORDER BY friend_id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let friends = rows
            .into_iter()
            .map(|row| serde_json::from_str(&row))
            .collect::<Result<Vec<_>, _>>()?;
        transaction.commit().await?;
        Ok(friends)
    }

    /// Saves a contact under a stable creation ID and optimistic edit revision.
    ///
    /// # Errors
    /// Rejects invalid metadata, stale differing edits and edits of deleted records.
    pub async fn save_friend(&self, request: &SaveFriend) -> Result<SavedFriend, PersistenceError> {
        if request.friend_id.is_nil()
            || request.expected_revision < 0
            || !request.details.is_valid()
        {
            return Err(rejected(
                "friend_invalid",
                "Contact metadata or revision is invalid.",
            ));
        }
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_complete_bootstrap_in_transaction(&mut transaction).await?;
        let existing = load(&mut transaction, request.friend_id).await?;
        let now = Utc::now();
        let friend = match existing {
            Some(friend)
                if request.expected_revision.checked_add(1) == Some(friend.revision)
                    && request.details == friend.details =>
            {
                friend
            }
            Some(mut friend)
                if request.expected_revision > 0
                    && request.expected_revision == friend.revision =>
            {
                friend.revision = friend
                    .revision
                    .checked_add(1)
                    .ok_or_else(|| rejected("friend_conflict", "Contact revision is exhausted."))?;
                friend.details.clone_from(&request.details);
                friend.updated_at = now;
                friend
            }
            None if request.expected_revision == 0 => SavedFriend {
                friend_id: request.friend_id,
                revision: 1,
                details: request.details.clone(),
                created_at: now,
                updated_at: now,
            },
            _ => {
                return Err(rejected(
                    "friend_conflict",
                    "Contact changed or was deleted. Reload before saving.",
                ));
            }
        };
        sqlx::query("INSERT INTO saved_friends(friend_id, friend_json) VALUES (?, ?) ON CONFLICT(friend_id) DO UPDATE SET friend_json = excluded.friend_json")
            .bind(friend.friend_id.to_string())
            .bind(serde_json::to_string(&friend)?)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(friend)
    }

    /// Deletes a saved contact without changing room admission or agent state.
    ///
    /// # Errors
    /// Rejects incomplete bootstrap and propagates storage failures.
    pub async fn delete_friend(&self, friend_id: Uuid) -> Result<bool, PersistenceError> {
        let mut transaction = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        require_complete_bootstrap_in_transaction(&mut transaction).await?;
        // Retain only the ID so a delayed creation retry cannot restore deleted metadata.
        let result = sqlx::query("UPDATE saved_friends SET friend_json = NULL WHERE friend_id = ? AND friend_json IS NOT NULL")
            .bind(friend_id.to_string())
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        Ok(result.rows_affected() != 0)
    }
}

async fn load(
    transaction: &mut Transaction<'_, Sqlite>,
    friend_id: Uuid,
) -> Result<Option<SavedFriend>, PersistenceError> {
    let raw: Option<Option<String>> =
        sqlx::query_scalar("SELECT friend_json FROM saved_friends WHERE friend_id = ?")
            .bind(friend_id.to_string())
            .fetch_optional(&mut **transaction)
            .await?;
    match raw {
        Some(Some(raw)) => Ok(Some(serde_json::from_str(&raw)?)),
        Some(None) => Err(rejected(
            "friend_conflict",
            "Contact was deleted. Create a new contact to add it again.",
        )),
        None => Ok(None),
    }
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.to_owned(),
    }
}
