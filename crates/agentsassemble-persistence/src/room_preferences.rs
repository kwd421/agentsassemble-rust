use agentsassemble_domain::{
    LOCAL_OPERATOR_USER_ID, Room, RoomSettings, RoomUserPreferences, RoomUserPreferencesPatch,
};
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    bootstrap::require_complete_bootstrap_in_transaction,
    human_session_authority::revalidate_human_session,
    room_user_identity::resolve_room_user_identity,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomPreferencesSnapshot {
    pub room_settings: RoomSettings,
    pub preferences: RoomUserPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRoomPreferencesDirectoryEntry {
    pub room: Room,
    pub room_settings: RoomSettings,
    pub preferences: RoomUserPreferences,
}

impl SqliteStore {
    /// Reads every room and the local user's preferences under one complete-bootstrap snapshot.
    ///
    /// Archived rooms remain present because this is the original server-wide settings branch,
    /// not the active-room membership projection.
    ///
    /// # Errors
    ///
    /// Fails when bootstrap integrity, stored room identity, JSON, or persistence is invalid.
    pub async fn local_room_preferences_directory(
        &self,
    ) -> Result<Vec<LocalRoomPreferencesDirectoryEntry>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        require_complete_bootstrap_in_transaction(&mut transaction).await?;
        let rows = sqlx::query(
            "SELECT rooms.room_id, rooms.room_json, rooms.settings_json, room_user_preferences.preferences_json FROM rooms LEFT JOIN room_user_preferences ON room_user_preferences.room_id = rooms.room_id AND room_user_preferences.user_id = ?",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .fetch_all(&mut *transaction)
        .await?;
        let mut entries = Vec::with_capacity(rows.len());
        for row in rows {
            let row_room_id = row.get::<String, _>("room_id");
            let room: Room = serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
            if room.room_id != row_room_id {
                return Err(invalid_stored_room());
            }
            let room_settings: RoomSettings =
                serde_json::from_str(row.get::<String, _>("settings_json").as_str())?;
            let preferences = row
                .get::<Option<String>, _>("preferences_json")
                .map_or_else(
                    || Ok(RoomUserPreferences::default()),
                    |encoded| serde_json::from_str(&encoded).map_err(PersistenceError::from),
                )?;
            entries.push(LocalRoomPreferencesDirectoryEntry {
                room,
                room_settings,
                preferences,
            });
        }
        entries.sort_by(|left, right| {
            right
                .room
                .updated_at
                .cmp(&left.room.updated_at)
                .then_with(|| left.room.room_id.cmp(&right.room.room_id))
        });
        transaction.commit().await?;
        Ok(entries)
    }

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

    /// Reads preferences only while the exact durable human session remains current.
    ///
    /// # Errors
    ///
    /// Fails when session provenance, membership, profile binding, stored JSON, or persistence is
    /// no longer valid.
    pub async fn human_session_room_preferences(
        &self,
        expected: &HumanSessionAuthorization,
    ) -> Result<RoomPreferencesSnapshot, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, expected, chrono::Utc::now()).await?;
        let principal = current.principal();
        let snapshot = load_room_preferences_snapshot(
            &mut transaction,
            &principal.principal_id,
            &principal.room_id,
        )
        .await?;
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
        let snapshot =
            update_room_preferences_in_transaction(&mut transaction, room_id, user_id, patch)
                .await?;
        transaction.commit().await?;
        Ok(snapshot)
    }

    /// Replaces preference fields only while the exact durable human session remains current.
    ///
    /// # Errors
    ///
    /// Fails without writing when session provenance, membership, profile binding, input, stored
    /// state, or persistence is no longer valid.
    pub async fn update_human_session_room_preferences(
        &self,
        expected: &HumanSessionAuthorization,
        patch: RoomUserPreferencesPatch,
    ) -> Result<RoomPreferencesSnapshot, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, expected, chrono::Utc::now()).await?;
        let principal = current.principal();
        let snapshot = update_room_preferences_in_transaction(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            patch,
        )
        .await?;
        transaction.commit().await?;
        Ok(snapshot)
    }
}

async fn update_room_preferences_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    user_id: &str,
    patch: RoomUserPreferencesPatch,
) -> Result<RoomPreferencesSnapshot, PersistenceError> {
    let current = load_room_preferences(transaction, user_id, room_id).await?;
    let preferences =
        current
            .apply_patch(patch)
            .map_err(|error| PersistenceError::CommandRejected {
                code: error.code,
                message: error.message,
            })?;
    sqlx::query(
        "INSERT INTO room_user_preferences(user_id, room_id, preferences_json) VALUES (?, ?, ?) ON CONFLICT(user_id, room_id) DO UPDATE SET preferences_json = excluded.preferences_json",
    )
    .bind(user_id)
    .bind(room_id)
    .bind(serde_json::to_string(&preferences)?)
    .execute(&mut **transaction)
    .await?;
    Ok(RoomPreferencesSnapshot {
        room_settings: load_room_settings(transaction, room_id).await?,
        preferences,
    })
}

fn invalid_stored_room() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: "Stored room authority is invalid.".to_owned(),
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
    use std::collections::BTreeMap;

    use agentsassemble_domain::{
        ChannelNotificationMode, ChannelPreference, LOCAL_OPERATOR_PARTICIPANT_ID,
        LOCAL_OPERATOR_USER_ID, Participant, RoomNotificationMode, RoomStatus,
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

    #[tokio::test]
    async fn directly_constructed_invalid_patch_cannot_create_stored_state() {
        let store = fixture().await;
        let invalid_patch = RoomUserPreferencesPatch {
            notifications: None,
            channel_settings: Some(BTreeMap::from([(
                "unsupported".to_owned(),
                ChannelPreference {
                    notifications: ChannelNotificationMode::Default,
                    last_read_at: "cursor".to_owned(),
                },
            )])),
        };

        assert!(matches!(
            store
                .update_room_preferences(
                    "general",
                    LOCAL_OPERATOR_USER_ID,
                    LOCAL_OPERATOR_PARTICIPANT_ID,
                    invalid_patch,
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "room_preferences_invalid",
                ..
            })
        ));
        let stored = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_user_preferences WHERE user_id = ? AND room_id = 'general'",
        )
        .bind(LOCAL_OPERATOR_USER_ID)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count preference rows: {error}"));
        assert_eq!(stored, 0);
    }

    #[tokio::test]
    async fn local_settings_directory_includes_archived_rooms_and_exact_preferences() {
        let store = fixture().await;
        store
            .create_room_for_local_operator(
                "1c740f9e-e228-4b53-a4e9-a859e901d23c",
                "archived-room",
                "Archived",
            )
            .await
            .unwrap_or_else(|error| panic!("create archived room fixture: {error}"));
        let encoded = sqlx::query_scalar::<_, String>(
            "SELECT room_json FROM rooms WHERE room_id = 'archived-room'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read archived room fixture: {error}"));
        let mut archived: agentsassemble_domain::Room = serde_json::from_str(&encoded)
            .unwrap_or_else(|error| panic!("decode archived room fixture: {error}"));
        archived.status = RoomStatus::Archived;
        sqlx::query("UPDATE rooms SET room_json = ? WHERE room_id = 'archived-room'")
            .bind(
                serde_json::to_string(&archived)
                    .unwrap_or_else(|error| panic!("encode archived room fixture: {error}")),
            )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("archive room fixture: {error}"));
        let patch = serde_json::from_value(json!({"notifications": "mute"}))
            .unwrap_or_else(|error| panic!("parse directory preference patch: {error}"));
        store
            .update_room_preferences(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                patch,
            )
            .await
            .unwrap_or_else(|error| panic!("write directory preferences: {error}"));

        let directory = store
            .local_room_preferences_directory()
            .await
            .unwrap_or_else(|error| panic!("read local settings directory: {error}"));
        assert_eq!(directory.len(), 2);
        let general = directory
            .iter()
            .find(|entry| entry.room.room_id == "general")
            .unwrap_or_else(|| panic!("general room missing"));
        assert_eq!(
            general.preferences.notifications,
            RoomNotificationMode::Mute
        );
        let archived = directory
            .iter()
            .find(|entry| entry.room.room_id == "archived-room")
            .unwrap_or_else(|| panic!("archived room missing"));
        assert_eq!(archived.room.status, RoomStatus::Archived);
        assert_eq!(
            archived.preferences,
            agentsassemble_domain::RoomUserPreferences::default()
        );

        sqlx::query("UPDATE local_bootstrap_authority SET initialization_digest = 'broken'")
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("corrupt directory bootstrap fixture: {error}"));
        assert!(matches!(
            store.local_room_preferences_directory().await,
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
