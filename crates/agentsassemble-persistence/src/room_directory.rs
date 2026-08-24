use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomEvent,
    RoomSettings, RoomStatus,
};
use chrono::Utc;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::{PersistenceError, SqliteStore, profile_store::load_local_operator_profile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredRoomSummary {
    pub room: Room,
    pub settings: RoomSettings,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoomCreateCommit {
    pub room: Room,
    pub settings: RoomSettings,
    pub events: Vec<RoomEvent>,
}

impl SqliteStore {
    /// Returns the durable identity of this database authority.
    ///
    /// # Errors
    ///
    /// Fails when the identity row is missing, malformed, or unreadable.
    pub async fn server_id(&self) -> Result<String, PersistenceError> {
        let value = sqlx::query_scalar::<_, String>(
            "SELECT value FROM runtime_metadata WHERE key = 'server_id'",
        )
        .fetch_optional(&self.pool)
        .await?
        .ok_or(PersistenceError::InvalidServerId)?;
        Uuid::parse_str(&value).map_err(|_| PersistenceError::InvalidServerId)?;
        Ok(value)
    }

    /// Reads canonical room and settings records in directory order.
    ///
    /// # Errors
    ///
    /// Fails on database errors, corrupt JSON, or a row whose embedded identity disagrees.
    pub async fn list_room_directory(
        &self,
        include_archived: bool,
    ) -> Result<Vec<StoredRoomSummary>, PersistenceError> {
        let rows = sqlx::query("SELECT room_id, room_json, settings_json FROM rooms")
            .fetch_all(&self.pool)
            .await?;
        let mut rooms = Vec::with_capacity(rows.len());
        for row in rows {
            let row_room_id = row.get::<String, _>("room_id");
            let room: Room = serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
            let settings: RoomSettings =
                serde_json::from_str(row.get::<String, _>("settings_json").as_str())?;
            if room.room_id != row_room_id {
                return Err(invalid_room_state());
            }
            if include_archived || room.status != RoomStatus::Archived {
                rooms.push(StoredRoomSummary { room, settings });
            }
        }
        rooms.sort_by(|left, right| {
            right
                .room
                .updated_at
                .cmp(&left.room.updated_at)
                .then_with(|| left.room.room_id.cmp(&right.room.room_id))
        });
        Ok(rooms)
    }

    /// Creates or idempotently updates one room under the local server operator.
    ///
    /// New room, settings, publication cursor, human membership, and creation event
    /// are one transaction. Existing rooms keep their UID, state, and membership.
    ///
    /// # Errors
    ///
    /// Fails on corrupt/missing authority, inactive local membership, or database errors.
    pub async fn create_room_for_local_operator(
        &self,
        room_id: &str,
        label: &str,
    ) -> Result<RoomCreateCommit, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query("SELECT room_json, settings_json FROM rooms WHERE room_id = ?")
            .bind(room_id)
            .fetch_optional(&mut *transaction)
            .await?;
        let now = Utc::now();
        let outcome = if let Some(row) = existing {
            let mut room: Room = serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
            let mut settings: RoomSettings =
                serde_json::from_str(row.get::<String, _>("settings_json").as_str())?;
            if room.room_id != room_id {
                return Err(invalid_room_state());
            }
            require_active_local_membership(&mut transaction, room_id).await?;
            label.clone_into(&mut room.label);
            label.clone_into(&mut settings.label);
            room.updated_at = now;
            sqlx::query("UPDATE rooms SET room_json = ?, settings_json = ? WHERE room_id = ?")
                .bind(serde_json::to_string(&room)?)
                .bind(serde_json::to_string(&settings)?)
                .bind(room_id)
                .execute(&mut *transaction)
                .await?;
            RoomCreateCommit {
                room,
                settings,
                events: Vec::new(),
            }
        } else {
            let profile = load_local_operator_profile(&mut transaction).await?;
            let room = Room::new(room_id.to_owned(), label.to_owned(), now);
            let settings = RoomSettings::defaults(label);
            let participant = Participant {
                room_id: room_id.to_owned(),
                participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
                display_name: profile.display_name,
                avatar_image_url: profile.avatar_image_url,
                participant_type: "human".to_owned(),
                status: ParticipantStatus::Joined,
                role: "host".to_owned(),
                owner_id: String::new(),
                muted: false,
                created_at: now,
                updated_at: now,
            };
            sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)")
                .bind(room_id)
                .bind(serde_json::to_string(&room)?)
                .bind(serde_json::to_string(&settings)?)
                .execute(&mut *transaction)
                .await?;
            sqlx::query(
                "INSERT INTO room_event_publication_cursors(room_id, published_seq) VALUES (?, 0)",
            )
            .bind(room_id)
            .execute(&mut *transaction)
            .await?;
            sqlx::query(
                "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
            )
            .bind(room_id)
            .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
            .bind(serde_json::to_string(&participant)?)
            .execute(&mut *transaction)
            .await?;
            let event = room_created_event(room_id, label, now);
            sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, 1, ?)")
                .bind(room_id)
                .bind(serde_json::to_string(&event)?)
                .execute(&mut *transaction)
                .await?;
            RoomCreateCommit {
                room,
                settings,
                events: vec![event],
            }
        };
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn require_active_local_membership(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
) -> Result<(), PersistenceError> {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = ? AND participant_id = ?",
    )
    .bind(room_id)
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or_else(inactive_membership)?;
    let participant: Participant = serde_json::from_str(&encoded)?;
    if participant.room_id != room_id
        || participant.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
        || participant.participant_type != "human"
        || participant.status != ParticipantStatus::Joined
    {
        return Err(inactive_membership());
    }
    Ok(())
}

fn room_created_event(room_id: &str, label: &str, now: chrono::DateTime<Utc>) -> RoomEvent {
    let mut extra = BTreeMap::new();
    extra.insert("label".to_owned(), Value::String(label.to_owned()));
    RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: 1,
        created_at: now,
        room_id: room_id.to_owned(),
        event_type: "room_created".to_owned(),
        actor: Actor {
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            participant_type: "human".to_owned(),
        },
        participant_id: None,
        participant_type: None,
        actor_id: Some(LOCAL_OPERATOR_PARTICIPANT_ID.to_owned()),
        actor_type: Some("human".to_owned()),
        display_name: None,
        content: None,
        message_kind: None,
        extra,
    }
}

fn inactive_membership() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "room_membership_inactive",
        message: "The local operator is not an active room participant.".to_owned(),
    }
}

fn invalid_room_state() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "invalid_state",
        message: "Stored room authority is invalid.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, UserProfilePatch,
    };

    use crate::SqliteStore;

    #[tokio::test]
    async fn room_creation_is_atomic_profile_derived_and_state_idempotent() {
        let store = fixture().await;
        let principal = local_principal("general");
        store
            .update_user_profile(
                &principal,
                UserProfilePatch {
                    display_name: Some("Directory Human".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("update profile fixture: {error}"));

        let created = store
            .create_room_for_local_operator("project-room", "Project Room")
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        assert_eq!(created.events.len(), 1);
        assert_eq!(created.events[0].event_type, "room_created");
        assert_eq!(created.events[0].seq, 1);
        let stable_uid = created.room.room_uid;
        let participant = store
            .participant("project-room", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read created membership: {error}"));
        assert_eq!(participant.display_name, "Directory Human");
        assert_eq!(participant.role, "host");

        let mut changed_membership = participant;
        changed_membership.role = "reviewer".to_owned();
        changed_membership.muted = true;
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
        )
        .bind(
            serde_json::to_string(&changed_membership)
                .unwrap_or_else(|error| panic!("encode changed membership: {error}")),
        )
        .bind("project-room")
        .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("change room-owned membership fields: {error}"));

        let retry = store
            .create_room_for_local_operator("project-room", "Renamed Project")
            .await
            .unwrap_or_else(|error| panic!("retry room creation: {error}"));
        assert_eq!(retry.room.room_uid, stable_uid);
        assert_eq!(retry.settings.label, "Renamed Project");
        assert!(retry.events.is_empty());
        let preserved = store
            .participant("project-room", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read preserved membership: {error}"));
        assert_eq!(preserved.role, "reviewer");
        assert!(preserved.muted);
        let snapshot = store
            .snapshot("project-room", 0, 20)
            .await
            .unwrap_or_else(|error| panic!("read created room snapshot: {error}"));
        assert_eq!(snapshot.events.len(), 1);
        assert_eq!(snapshot.last_seq, 1);
    }

    #[tokio::test]
    async fn server_identity_survives_reopen_and_corruption_never_creates_a_room() {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create directory fixture: {error}"));
        let path = directory.path().join("runtime.sqlite3");
        let store = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("open directory store: {error}"));
        bootstrap(&store).await;
        let server_id = store
            .server_id()
            .await
            .unwrap_or_else(|error| panic!("read server id: {error}"));
        sqlx::query("UPDATE user_profiles SET profile_json = 'not-json' WHERE user_id = ?")
            .bind(LOCAL_OPERATOR_USER_ID)
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("corrupt profile fixture: {error}"));
        assert!(
            store
                .create_room_for_local_operator("must-not-exist", "Invalid")
                .await
                .is_err()
        );
        assert!(!store.room_exists("must-not-exist").await.unwrap_or(true));
        drop(store);

        let reopened = SqliteStore::open_path(&path)
            .await
            .unwrap_or_else(|error| panic!("reopen directory store: {error}"));
        assert_eq!(
            reopened
                .server_id()
                .await
                .unwrap_or_else(|error| panic!("read reopened server id: {error}")),
            server_id
        );
    }

    async fn fixture() -> SqliteStore {
        let url = format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );
        let store = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open room directory fixture: {error}"));
        bootstrap(&store).await;
        store
    }

    async fn bootstrap(store: &SqliteStore) {
        store
            .bootstrap_local_authority("c58407c8-bf45-4916-a84e-579e9331c512", "SeiNel")
            .await
            .unwrap_or_else(|error| panic!("bootstrap room directory identity: {error}"));
        store
            .create_room_for_local_operator("general", "General")
            .await
            .unwrap_or_else(|error| panic!("create room directory fixture: {error}"));
    }

    fn local_principal(room_id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "SeiNel".to_owned(),
            room_id: room_id.to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        }
    }
}
