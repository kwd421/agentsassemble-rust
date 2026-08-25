use agentsassemble_domain::{RoomSettings, RoomUserPreferences, RoomUserPreferencesPatch};
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore, room_user_identity::resolve_room_user_identity};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomPreferencesSnapshot {
    pub room_settings: RoomSettings,
    pub preferences: RoomUserPreferences,
}

impl SqliteStore {
    /// Reads one current human's preferences and the canonical room settings in one transaction.
    ///
    /// # Errors
    ///
    /// Fails when identity is stale, stored JSON is invalid, or persistence is unavailable.
    pub async fn room_preferences(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
    ) -> Result<RoomPreferencesSnapshot, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        let snapshot = load_room_preferences_snapshot(&mut transaction, user_id, room_id).await?;
        transaction.commit().await?;
        Ok(snapshot)
    }

    /// Replaces the supplied preference fields for one current human and returns one snapshot.
    ///
    /// # Errors
    ///
    /// Fails without writing when identity is stale, stored state is invalid, or the transaction
    /// cannot commit.
    pub async fn update_room_preferences(
        &self,
        room_id: &str,
        user_id: &str,
        participant_id: &str,
        patch: RoomUserPreferencesPatch,
    ) -> Result<RoomPreferencesSnapshot, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        resolve_room_user_identity(&mut transaction, room_id, user_id, participant_id).await?;
        let current = load_room_preferences(&mut transaction, user_id, room_id).await?;
        let preferences = current.apply_patch(patch);
        sqlx::query(
            "INSERT INTO room_user_preferences(user_id, room_id, preferences_json) VALUES (?, ?, ?) ON CONFLICT(user_id, room_id) DO UPDATE SET preferences_json = excluded.preferences_json",
        )
        .bind(user_id)
        .bind(room_id)
        .bind(serde_json::to_string(&preferences)?)
        .execute(&mut *transaction)
        .await?;
        let room_settings = load_room_settings(&mut transaction, room_id).await?;
        transaction.commit().await?;
        Ok(RoomPreferencesSnapshot {
            room_settings,
            preferences,
        })
    }
}

pub(crate) async fn load_room_preferences(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    room_id: &str,
) -> Result<RoomUserPreferences, PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT preferences_json FROM room_user_preferences WHERE user_id = ? AND room_id = ?",
    )
    .bind(user_id)
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?;
    encoded.map_or_else(
        || Ok(RoomUserPreferences::default()),
        |value| serde_json::from_str(&value).map_err(PersistenceError::from),
    )
}

async fn load_room_preferences_snapshot(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    room_id: &str,
) -> Result<RoomPreferencesSnapshot, PersistenceError> {
    Ok(RoomPreferencesSnapshot {
        room_settings: load_room_settings(transaction, room_id).await?,
        preferences: load_room_preferences(transaction, user_id, room_id).await?,
    })
}

async fn load_room_settings(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<RoomSettings, PersistenceError> {
    let encoded =
        sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_one(&mut **transaction)
            .await?;
    Ok(serde_json::from_str(&encoded)?)
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant, RoomNotificationMode,
        RoomUserPreferencesPatch,
    };
    use serde_json::json;

    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn preferences_are_isolated_by_canonical_user_and_room() {
        let store = fixture().await;
        add_second_human(&store).await;
        let local_patch: RoomUserPreferencesPatch = serde_json::from_value(json!({
            "notifications": "mute",
            "channel_settings": {
                "lobby": {"notifications": "mentions", "last_read_at": "local-cursor"}
            }
        }))
        .unwrap_or_else(|error| panic!("parse local patch: {error}"));
        let local = store
            .update_room_preferences(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                local_patch,
            )
            .await
            .unwrap_or_else(|error| panic!("write local preferences: {error}"));
        assert_eq!(
            serde_json::to_value(&local.preferences)
                .unwrap_or_else(|error| panic!("serialize local preferences: {error}")),
            json!({
                "notifications": "mute",
                "channel_settings": {
                    "lobby": {"notifications": "mentions", "last_read_at": "local-cursor"}
                }
            })
        );
        assert_eq!(local.room_settings.label, "General");

        let second_patch = serde_json::from_value(json!({"notifications": "all"}))
            .unwrap_or_else(|error| panic!("parse second patch: {error}"));
        store
            .update_room_preferences("general", "user-2", "participant-2", second_patch)
            .await
            .unwrap_or_else(|error| panic!("write second preferences: {error}"));
        let local_again = store
            .room_preferences(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await
            .unwrap_or_else(|error| panic!("read local preferences: {error}"));
        let second = store
            .room_preferences("general", "user-2", "participant-2")
            .await
            .unwrap_or_else(|error| panic!("read second preferences: {error}"));
        assert_eq!(local_again.preferences, local.preferences);
        assert_eq!(second.preferences.notifications, RoomNotificationMode::All);
        assert!(second.preferences.channel_settings.is_empty());
    }

    #[tokio::test]
    async fn corrupt_preferences_are_not_replaced_by_defaults() {
        let store = fixture().await;
        sqlx::query(
            "INSERT INTO room_user_preferences(user_id, room_id, preferences_json) VALUES (?, 'general', '{}')",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert corrupt preferences: {error}"));
        assert!(matches!(
            store
                .room_preferences(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                )
                .await,
            Err(PersistenceError::Json(_))
        ));
    }

    async fn fixture() -> SqliteStore {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("e63d317f-b023-45c2-906d-9e145535c764", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap local authority: {error}"));
        store
            .create_room_for_local_operator(
                "0c09d218-5048-4409-987c-e55cc5bd104b",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        store
    }

    async fn add_second_human(store: &SqliteStore) {
        let profile_json = sqlx::query_scalar::<_, String>(
            "SELECT profile_json FROM user_profiles WHERE user_id = ?",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read profile JSON: {error}"));
        sqlx::query(
            "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES ('user-2', 'participant-2', ?)",
        )
        .bind(profile_json)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert second profile: {error}"));
        let participant_json = sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read participant JSON: {error}"));
        let mut participant: Participant = serde_json::from_str(&participant_json)
            .unwrap_or_else(|error| panic!("decode participant: {error}"));
        participant.participant_id = "participant-2".to_owned();
        participant.display_name = "Second".to_owned();
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', 'participant-2', ?)",
        )
        .bind(serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode participant: {error}")))
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert second participant: {error}"));
    }
}
