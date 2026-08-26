use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ParticipantStatus,
};
use sqlx::{Sqlite, Transaction};

use crate::{
    PersistenceError, SqliteStore, authority::load_active_participant,
    bootstrap::require_complete_bootstrap_in_transaction, profile_store::load_profile_for_identity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomUserIdentity {
    pub room_id: String,
    pub user_id: String,
    pub participant_id: String,
}

impl SqliteStore {
    /// Resolves one current room human through the canonical profile binding.
    ///
    /// # Errors
    ///
    /// Fails when the room or membership is inactive, the participant is not human, or the
    /// profile-to-participant binding is missing or inconsistent.
    pub async fn authorize_room_user(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
    ) -> Result<RoomUserIdentity, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let identity =
            resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        transaction.commit().await?;
        Ok(identity)
    }

    /// Resolves the only current room manager from durable local-bootstrap authority.
    ///
    /// # Errors
    ///
    /// Fails when room-user identity is stale or when the full bootstrap integrity contract no
    /// longer proves the exact local operator binding.
    pub async fn authorize_local_room_manager(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
    ) -> Result<RoomUserIdentity, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let identity =
            resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        require_current_local_room_manager(&mut transaction, &identity).await?;
        transaction.commit().await?;
        Ok(identity)
    }
}

pub(crate) async fn resolve_room_user_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    user_id: &str,
    participant_id: &str,
) -> Result<RoomUserIdentity, PersistenceError> {
    let participant = load_active_participant(transaction, room_id, participant_id).await?;
    if participant.status != ParticipantStatus::Joined || participant.participant_type != "human" {
        return Err(rejected(
            "session_revoked",
            "Room preferences require a current human participant.",
        ));
    }
    load_profile_for_identity(transaction, user_id, participant_id).await?;
    Ok(RoomUserIdentity {
        room_id: room_id.to_owned(),
        user_id: user_id.to_owned(),
        participant_id: participant_id.to_owned(),
    })
}

pub(crate) async fn require_current_local_room_manager(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: &RoomUserIdentity,
) -> Result<(), PersistenceError> {
    if identity.user_id != LOCAL_OPERATOR_USER_ID
        || identity.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
    {
        return Err(rejected(
            "permission_denied",
            "Only the current local room manager may manage this room.",
        ));
    }
    require_complete_bootstrap_in_transaction(transaction).await?;
    Ok(())
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{PersistenceError, SqliteStore};
    use agentsassemble_domain::{
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant, ParticipantStatus,
    };

    #[tokio::test]
    async fn room_user_identity_uses_profile_binding_not_participant_owner_id() {
        let store = fixture().await;
        let participant = store
            .participant("general", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read local participant: {error}"));
        assert!(participant.owner_id.is_empty());

        let identity = store
            .authorize_room_user(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("resolve room user: {error}"));
        assert_eq!(identity.user_id, LOCAL_OPERATOR_USER_ID);
        assert_eq!(identity.participant_id, LOCAL_OPERATOR_PARTICIPANT_ID);
    }

    #[tokio::test]
    async fn current_authorization_rejects_membership_revoked_after_an_earlier_read() {
        let store = fixture().await;
        store
            .authorize_room_user(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("authorize current room user: {error}"));
        let mut participant = store
            .participant("general", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read current participant: {error}"));
        participant.status = ParticipantStatus::Left;
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
        )
        .bind(
            serde_json::to_string(&participant)
                .unwrap_or_else(|error| panic!("encode revoked participant: {error}")),
        )
        .bind(&participant.room_id)
        .bind(&participant.participant_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("revoke current participant: {error}"));

        assert!(matches!(
            store
                .authorize_room_user(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "session_revoked",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn nonhuman_or_mismatched_profile_identity_fails_closed() {
        let store = fixture().await;
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read participant JSON: {error}"));
        let mut participant: Participant = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode participant: {error}"));
        participant.participant_type = "agent".to_owned();
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode participant: {error}")))
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("write nonhuman participant: {error}"));
        assert!(matches!(
            store
                .authorize_room_user(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "session_revoked",
                ..
            })
        ));

        participant.participant_type = "human".to_owned();
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode participant: {error}")))
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("restore human participant: {error}"));
        sqlx::query(
            "UPDATE user_profiles SET participant_id = 'different-participant' WHERE user_id = ?",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("break profile binding: {error}"));
        assert!(matches!(
            store
                .authorize_room_user(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "profile_authority_mismatch",
                ..
            })
        ));
    }

    #[tokio::test]
    async fn manager_authority_runs_the_full_bootstrap_integrity_check() {
        let store = fixture().await;
        store
            .authorize_local_room_manager(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("resolve local manager: {error}"));

        sqlx::query("UPDATE local_bootstrap_authority SET initialization_digest = 'broken'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("corrupt bootstrap digest: {error}"));
        assert!(matches!(
            store
                .authorize_local_room_manager(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "bootstrap_repair_required",
                ..
            })
        ));
    }

    async fn fixture() -> SqliteStore {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("baef1a5c-c6f6-4d7b-8f5c-e500ef84a813", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap local authority: {error}"));
        store
            .create_room_for_local_operator(
                "897948ca-a367-4741-8fe4-9194086a0a51",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        store
    }
}
