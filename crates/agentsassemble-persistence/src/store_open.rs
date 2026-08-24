use std::path::Path;

use agentsassemble_domain::{Participant, Room, RoomSettings};
use sqlx::{SqlitePool, sqlite::SqlitePoolOptions};

use crate::{
    PersistenceError, SqliteStore, bootstrap::InitialRoom, database_target::PreparedDatabase,
};

impl SqliteStore {
    /// Opens an explicit `SQLite` URL and verifies its ownership marker.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the store cannot be owned safely.
    pub async fn open(database_url: &str) -> Result<Self, PersistenceError> {
        Self::open_prepared(PreparedDatabase::from_url(database_url)?, None).await
    }

    /// Opens a file authority without interpreting path characters as URL options.
    ///
    /// # Errors
    ///
    /// Returns a database or authority error when the path cannot be owned safely.
    pub async fn open_path(path: &Path) -> Result<Self, PersistenceError> {
        Self::open_prepared(PreparedDatabase::from_path(path)?, None).await
    }

    /// Opens one file authority and atomically installs its initial product state.
    ///
    /// # Errors
    ///
    /// Returns a database, authority, or initial-state validation error.
    pub async fn open_path_with_initial_room(
        path: &Path,
        room: &Room,
        settings: &RoomSettings,
        participant: &Participant,
    ) -> Result<Self, PersistenceError> {
        Self::open_prepared(
            PreparedDatabase::from_path(path)?,
            Some(InitialRoom {
                room,
                settings,
                participant,
            }),
        )
        .await
    }

    async fn open_prepared(
        prepared: PreparedDatabase,
        initial_room: Option<InitialRoom<'_>>,
    ) -> Result<Self, PersistenceError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(prepared.options.clone().create_if_missing(true))
            .await?;
        if prepared.identity.is_some() {
            acquire_file_authority_lock(&pool).await?;
        }
        prepared.revalidate()?;
        let empty_authority = !prepared.created && database_is_empty(&pool).await?;
        let store = Self {
            pool,
            _writer_lease: prepared.writer_lease,
            _database_identity: prepared.identity,
            created: prepared.created || empty_authority,
        };
        if store.created {
            store.initialize(initial_room).await?;
        } else {
            store.verify_owner().await?;
            if let Some(initial_room) = initial_room {
                store.verify_initial_product_state(initial_room).await?;
            }
        }
        Ok(store)
    }
}

async fn database_is_empty(pool: &SqlitePool) -> Result<bool, PersistenceError> {
    let objects = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
    )
    .fetch_one(pool)
    .await?;
    Ok(objects == 0)
}

async fn acquire_file_authority_lock(pool: &SqlitePool) -> Result<(), PersistenceError> {
    let mut connection = pool.acquire().await?;
    sqlx::query("BEGIN EXCLUSIVE")
        .execute(&mut *connection)
        .await?;
    sqlx::query("COMMIT").execute(&mut *connection).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;

    use agentsassemble_domain::{
        LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
    };
    use chrono::Utc;

    use super::SqliteStore;

    fn initial_room() -> (Room, RoomSettings, Participant) {
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "general".to_owned(), now);
        let settings = RoomSettings::defaults("general");
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Operator".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        (room, settings, participant)
    }

    #[tokio::test]
    async fn interrupted_empty_file_retries_complete_bootstrap() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let file = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(&path)
            .unwrap_or_else(|error| panic!("create interrupted database file: {error}"));
        crate::private_fs::secure_file(&file)
            .unwrap_or_else(|error| panic!("secure interrupted database file: {error}"));
        drop(file);

        let (room, settings, participant) = initial_room();
        let restored =
            SqliteStore::open_path_with_initial_room(&path, &room, &settings, &participant)
                .await
                .unwrap_or_else(|error| panic!("retry interrupted bootstrap: {error}"));
        assert!(restored.was_created());
        assert_eq!(
            restored
                .list_room_directory(true)
                .await
                .unwrap_or_else(|error| panic!("restored directory: {error}"))
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn fresh_authority_commits_schema_identity_and_initial_room_together() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let (room, settings, participant) = initial_room();
        let store = SqliteStore::open_path_with_initial_room(&path, &room, &settings, &participant)
            .await
            .unwrap_or_else(|error| panic!("initialize authority: {error}"));
        assert!(store.was_created());
        assert!(
            uuid::Uuid::parse_str(
                &store
                    .server_id()
                    .await
                    .unwrap_or_else(|error| panic!("server id: {error}"))
            )
            .is_ok()
        );
        let directory = store
            .list_room_directory(true)
            .await
            .unwrap_or_else(|error| panic!("room directory: {error}"));
        assert_eq!(directory.len(), 1);
        assert_eq!(directory[0].room.room_id, "general");
    }

    #[tokio::test]
    async fn schema_only_authority_is_not_reinterpreted_as_a_complete_bootstrap() {
        let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("tempdir: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let schema_only = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("initialize schema: {error}"));
        drop(schema_only);

        let (room, settings, participant) = initial_room();
        assert!(matches!(
            SqliteStore::open_path_with_initial_room(&path, &room, &settings, &participant).await,
            Err(crate::PersistenceError::InitializationNotAllowed)
        ));

        let incomplete_path = directory.path().join("incomplete-current.sqlite3");
        let complete = SqliteStore::open_path_with_initial_room(
            &incomplete_path,
            &room,
            &settings,
            &participant,
        )
        .await
        .unwrap_or_else(|error| panic!("initialize complete authority: {error}"));
        sqlx::query("DELETE FROM participants WHERE room_id = ? AND participant_id = ?")
            .bind(&room.room_id)
            .bind(&participant.participant_id)
            .execute(&complete.pool)
            .await
            .unwrap_or_else(|error| panic!("remove required participant: {error}"));
        drop(complete);
        assert!(matches!(
            SqliteStore::open_path_with_initial_room(
                &incomplete_path,
                &room,
                &settings,
                &participant
            )
            .await,
            Err(crate::PersistenceError::InitializationNotAllowed)
        ));
    }
}
