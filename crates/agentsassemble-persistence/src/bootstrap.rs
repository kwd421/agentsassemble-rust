use agentsassemble_domain::{Participant, Room, RoomSettings, validate_room_id};
use sqlx::{Sqlite, Transaction};

use crate::{PersistenceError, SqliteStore, sqlite::SCHEMA_OWNER};

#[derive(Clone, Copy)]
pub(crate) struct InitialRoom<'a> {
    pub room: &'a Room,
    pub settings: &'a RoomSettings,
    pub participant: &'a Participant,
}

impl SqliteStore {
    pub(crate) async fn initialize(
        &self,
        initial_room: Option<InitialRoom<'_>>,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        for statement in crate::schema::statements() {
            sqlx::query(*statement).execute(&mut *transaction).await?;
        }
        install_metadata(&mut transaction).await?;
        if let Some(initial_room) = initial_room {
            insert_initial_room(&mut transaction, initial_room).await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Inserts an explicit room fixture and its host participant if absent.
    ///
    /// # Errors
    ///
    /// Returns a persistence or serialization error; partial inserts roll back.
    pub async fn initialize_room(
        &self,
        room: &Room,
        settings: &RoomSettings,
        participant: &Participant,
    ) -> Result<(), PersistenceError> {
        if !self.created {
            return Err(PersistenceError::InitializationNotAllowed);
        }
        let mut transaction = self.pool.begin().await?;
        require_empty_product_state(&mut transaction).await?;
        insert_initial_room(
            &mut transaction,
            InitialRoom {
                room,
                settings,
                participant,
            },
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub(crate) async fn restore_missing_initial_room(
        &self,
        initial_room: InitialRoom<'_>,
    ) -> Result<(), PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let room_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rooms")
            .fetch_one(&mut *transaction)
            .await?;
        if room_count != 0 {
            transaction.commit().await?;
            return Ok(());
        }
        let profile_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_profiles")
            .fetch_one(&mut *transaction)
            .await?;
        if profile_count != 0 {
            transaction.commit().await?;
            return Ok(());
        }
        require_empty_product_state(&mut transaction).await?;
        insert_initial_room(&mut transaction, initial_room).await?;
        transaction.commit().await?;
        Ok(())
    }
}

async fn install_metadata(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    let owner = sqlx::query_scalar::<_, String>(
        "SELECT value FROM runtime_metadata WHERE key = 'schema_owner'",
    )
    .fetch_optional(&mut **transaction)
    .await?;
    match owner {
        Some(owner) if owner != SCHEMA_OWNER => {
            return Err(PersistenceError::AuthorityConflict(owner));
        }
        Some(_) => {}
        None => {
            sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('schema_owner', ?)")
                .bind(SCHEMA_OWNER)
                .execute(&mut **transaction)
                .await?;
        }
    }
    sqlx::query("INSERT INTO runtime_metadata(key, value) VALUES ('server_id', ?)")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO runtime_metadata(key, value) VALUES ('schema_version', ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(crate::migration::CURRENT_SCHEMA_VERSION.to_string())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn require_empty_product_state(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), PersistenceError> {
    let existing_rows = sqlx::query_scalar::<_, i64>(
        "SELECT (SELECT COUNT(*) FROM rooms) + (SELECT COUNT(*) FROM participants) + (SELECT COUNT(*) FROM user_profiles)",
    )
    .fetch_one(&mut **transaction)
    .await?;
    if existing_rows == 0 {
        Ok(())
    } else {
        Err(PersistenceError::InitializationNotAllowed)
    }
}

async fn insert_initial_room(
    transaction: &mut Transaction<'_, Sqlite>,
    initial: InitialRoom<'_>,
) -> Result<(), PersistenceError> {
    let room_id = validate_room_id(&initial.room.room_id).map_err(|error| {
        PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        }
    })?;
    if initial.participant.room_id != room_id || initial.settings.label != initial.room.label {
        return Err(PersistenceError::InitializationNotAllowed);
    }
    sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)")
        .bind(&room_id)
        .bind(serde_json::to_string(initial.room)?)
        .bind(serde_json::to_string(initial.settings)?)
        .execute(&mut **transaction)
        .await?;
    sqlx::query("INSERT INTO room_event_publication_cursors(room_id, published_seq) VALUES (?, 0)")
        .bind(&room_id)
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
    )
    .bind(&initial.participant.room_id)
    .bind(&initial.participant.participant_id)
    .bind(serde_json::to_string(initial.participant)?)
    .execute(&mut **transaction)
    .await?;
    crate::profile_store::insert_initial_local_profile(transaction, initial.participant).await?;
    Ok(())
}
