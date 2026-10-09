//! Account-deleted hosting is recoverable only by explicit fresh registration.
use crate::{MemberRemovalKey, PersistenceError, SqliteStore};
use sqlx::Row;

impl SqliteStore {
    /// Reads the explicit fresh-registration precondition without changing the old fence.
    /// # Errors
    /// Refuses other demotion reasons, removed persons and unfinished live effects.
    pub async fn require_fresh_host_registration(
        &self,
        issuer: &str,
        person: &str,
    ) -> Result<(), PersistenceError> {
        let mut tx = self.pool.begin().await?;
        let reason: Option<String> = sqlx::query_scalar(
            "SELECT value FROM runtime_metadata WHERE key='hosting_restriction'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM central_member_removals WHERE phase!='complete')",
        )
        .fetch_one(&mut *tx)
        .await?;
        if reason.as_deref() != Some("account_deleted")
            || pending
            || !crate::central_member_removal::person_is_live(&mut tx, issuer, person).await?
        {
            return Err(invalid());
        }
        tx.commit().await?;
        Ok(())
    }
    /// Applies the pinned central terminal response for this exact current incarnation.
    /// # Errors
    /// A stale tuple, invalid identity or checkpoint error leaves authority unchanged.
    pub async fn account_deleted_host(
        &self,
        issuer: &str,
        person: &str,
        epoch: &str,
    ) -> Result<MemberRemovalKey, PersistenceError> {
        let origin = url::Url::parse(issuer).map_err(|_| invalid())?;
        if origin.origin().ascii_serialization() != issuer
            || person.is_empty()
            || person.len() > 256
            || person.chars().any(char::is_control)
            || epoch.is_empty()
        {
            return Err(invalid());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        let stored: Option<String> = sqlx::query_scalar(
            "SELECT value FROM runtime_metadata WHERE key='central_registration_epoch'",
        )
        .fetch_optional(&mut *tx)
        .await?;
        if stored.as_deref() != Some(epoch) {
            return Err(invalid());
        }
        let binding=sqlx::query("SELECT b.user_id,p.participant_id FROM central_identity_bindings b JOIN user_profiles p USING(user_id) WHERE b.issuer=? AND b.person_id=?")
            .bind(issuer).bind(person).fetch_optional(&mut *tx).await?;
        let user: Option<String> = binding.as_ref().map(|r| r.try_get("user_id")).transpose()?;
        let actor: Option<String> = binding
            .as_ref()
            .map(|r| r.try_get("participant_id"))
            .transpose()?;
        sqlx::query("INSERT INTO central_member_removals(issuer,person_id,user_id,participant_id,request_id,phase,schema_revision) VALUES (?,?,?,?,'central-account-deleted-host-response','owner_sessions',87) ON CONFLICT(issuer,person_id) DO NOTHING")
            .bind(issuer).bind(person).bind(user).bind(actor).execute(&mut *tx).await?;
        sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES ('hosting_restriction','account_deleted') ON CONFLICT(key) DO UPDATE SET value='account_deleted' WHERE value='account_deleted'")
            .execute(&mut *tx).await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(MemberRemovalKey {
            issuer: issuer.into(),
            person_id: person.into(),
        })
    }

    /// Only the local administrator's explicit new-account registration may clear this reason.
    /// # Errors
    /// Rejects permanent demotion, an old epoch/person, unsettled effects or concurrent change.
    pub async fn register_after_account_deletion(
        &self,
        issuer: &str,
        person: &str,
        old_epoch: &str,
        new_epoch: &str,
    ) -> Result<(), PersistenceError> {
        if new_epoch.is_empty()
            || new_epoch.len() > 128
            || old_epoch == new_epoch
            || person.is_empty()
            || person.len() > 256
        {
            return Err(invalid());
        }
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::central_member_removal::require_schema(&mut tx).await?;
        if !crate::central_member_removal::person_is_live(&mut tx, issuer, person).await? {
            return Err(invalid());
        }
        let pending: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM central_member_removals WHERE phase!='complete')",
        )
        .fetch_one(&mut *tx)
        .await?;
        if pending {
            return Err(invalid());
        }
        let removed=sqlx::query("DELETE FROM runtime_metadata WHERE key='hosting_restriction' AND value='account_deleted' AND (SELECT value FROM runtime_metadata WHERE key='central_registration_epoch')=?")
            .bind(old_epoch).execute(&mut *tx).await?;
        if removed.rows_affected() != 1 {
            return Err(invalid());
        }
        sqlx::query("UPDATE runtime_metadata SET value=? WHERE key='central_registration_epoch'")
            .bind(new_epoch)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE member_projection_outbox SET parked=2 WHERE registration_epoch!=?")
            .bind(new_epoch)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.notify_room_directory_changed();
        Ok(())
    }
}
fn invalid() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "account_deleted_host_conflict".into(),
        message: "Account-deleted host authority changed or is not settled.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn deleted_host_restart_fresh_cas_and_permanent_reason_preserve_local_authority()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("host.sqlite");
        let store = SqliteStore::open_path(&path).await?;
        store
            .bootstrap_local_authority(
                "dacd9522-fd59-47f3-91f5-9e600959ab98",
                "Independent operator",
            )
            .await?;
        store.set_registration_epoch(Some("old")).await?;
        let identity = store.host_identity().await?.server_id().to_string();
        assert!(
            store
                .account_deleted_host("https://central.example", "deleted-person", "wrong")
                .await
                .is_err()
        );
        assert!(store.hosting_restriction().await?.is_none());
        let key = store
            .account_deleted_host("https://central.example", "deleted-person", "old")
            .await?;
        assert!(store.set_registration_epoch(Some("new")).await.is_err());
        assert!(
            store
                .require_fresh_host_registration("https://central.example", "fresh-person")
                .await
                .is_err()
        );
        settle_empty(&store, &key).await?;
        assert_eq!(
            store.local_operator_profile().await?.display_name,
            "Independent operator"
        );
        store.close().await?;
        let store = SqliteStore::open_path(&path).await?;
        assert_eq!(
            store.hosting_restriction().await?.as_deref(),
            Some("account_deleted")
        );
        assert_eq!(store.host_identity().await?.server_id(), identity);
        for (person, old, new) in [
            ("deleted-person", "old", "new"),
            ("fresh-person", "wrong", "new"),
            ("fresh-person", "old", "old"),
        ] {
            assert!(
                store
                    .register_after_account_deletion("https://central.example", person, old, new)
                    .await
                    .is_err()
            );
        }
        store
            .require_fresh_host_registration("https://central.example", "fresh-person")
            .await?;
        store
            .register_after_account_deletion(
                "https://central.example",
                "fresh-person",
                "old",
                "new",
            )
            .await?;
        assert!(store.hosting_restriction().await?.is_none());
        assert_eq!(store.registration_epoch().await?.as_deref(), Some("new"));
        assert_eq!(
            store.local_operator_profile().await?.display_name,
            "Independent operator"
        );
        let key = store
            .account_deleted_host("https://central.example", "second-deleted", "new")
            .await?;
        settle_empty(&store, &key).await?;
        store.restrict_hosting(true).await?;
        assert_eq!(
            store.hosting_restriction().await?.as_deref(),
            Some("retired")
        );
        assert!(
            store
                .register_after_account_deletion(
                    "https://central.example",
                    "fresh-again",
                    "new",
                    "next"
                )
                .await
                .is_err()
        );
        Ok(())
    }
    async fn settle_empty(
        store: &SqliteStore,
        key: &MemberRemovalKey,
    ) -> Result<(), PersistenceError> {
        for _ in 0..100 {
            match store.member_removal_phase(key).await?.as_str() {
                "complete" => return Ok(()),
                "owner_sessions" | "owner_pairings" | "human_sessions" => {
                    store.advance_member_removal_authority(key).await?;
                }
                "companions" => {
                    store.advance_member_removal_companions(key).await?;
                }
                "rooms" => {
                    assert!(store.advance_member_removal_room(key).await?.is_none());
                }
                "effects" | "effect_owners" | "effect_pairings" | "effect_humans"
                | "effect_companions" => {
                    let page = store.member_removal_effect(key).await?;
                    assert!(page.cleanup.is_none());
                    store.finish_member_removal_effect(key, &page).await?;
                }
                _ => {
                    store.advance_member_removal_snapshots(key).await?;
                }
            }
        }
        Err(invalid())
    }
}
