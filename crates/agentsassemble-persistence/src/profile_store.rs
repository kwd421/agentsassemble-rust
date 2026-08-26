use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    Participant, ParticipantStatus, Room, RoomEvent, RoomStatus, UserProfile, UserProfilePatch,
    avatar_attachment_id,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{Row, Sqlite, SqliteConnection, Transaction};
use uuid::Uuid;

use crate::{
    PersistenceError, SqliteStore,
    authority::authorize_session,
    profile_attachments::{authorize_profile_avatar, replace_profile_avatar},
};

#[derive(Debug, Clone, PartialEq)]
pub struct ProfileUpdateOutcome {
    pub profile: UserProfile,
    pub events: Vec<RoomEvent>,
}

#[derive(Clone, Copy)]
pub(crate) struct ProfileIdentity<'a> {
    pub(crate) user_id: &'a str,
    pub(crate) participant_id: &'a str,
}

impl<'a> ProfileIdentity<'a> {
    fn from_principal(principal: &'a AuthenticatedPrincipal) -> Self {
        Self {
            user_id: &principal.principal_id,
            participant_id: &principal.participant_id,
        }
    }

    pub(crate) const fn local_operator() -> Self {
        Self {
            user_id: LOCAL_OPERATOR_USER_ID,
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID,
        }
    }
}

impl SqliteStore {
    /// Reads the authenticated human profile from its server-wide authority.
    ///
    /// # Errors
    ///
    /// Fails when the room credential is stale or the profile authority is missing/corrupt.
    pub async fn user_profile(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<UserProfile, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        let profile =
            load_profile(&mut transaction, ProfileIdentity::from_principal(principal)).await?;
        transaction.commit().await?;
        Ok(profile)
    }

    /// Reads the bootstrapped local human profile without inventing a room membership.
    ///
    /// # Errors
    ///
    /// Fails when local bootstrap is incomplete or the profile authority is corrupt.
    pub async fn local_operator_profile(&self) -> Result<UserProfile, PersistenceError> {
        self.require_local_bootstrap_complete().await?;
        let mut transaction = self.pool.begin().await?;
        let profile = load_profile(&mut transaction, ProfileIdentity::local_operator()).await?;
        transaction.commit().await?;
        Ok(profile)
    }

    /// Atomically saves one profile revision and every current human room projection.
    ///
    /// # Errors
    ///
    /// Fails closed on stale authority, foreign avatar references, corrupt state, or `SQLite` errors.
    pub async fn update_user_profile(
        &self,
        principal: &AuthenticatedPrincipal,
        patch: UserProfilePatch,
    ) -> Result<ProfileUpdateOutcome, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_session(&mut transaction, principal).await?;
        let outcome = update_profile_in_transaction(
            &mut transaction,
            ProfileIdentity::from_principal(principal),
            patch,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Updates the server-wide local human profile and all existing room projections.
    ///
    /// # Errors
    ///
    /// Fails when local bootstrap is incomplete, profile authority is corrupt, or projection fails.
    pub async fn update_local_operator_profile(
        &self,
        patch: UserProfilePatch,
    ) -> Result<ProfileUpdateOutcome, PersistenceError> {
        self.require_local_bootstrap_complete().await?;
        let mut transaction = self.pool.begin().await?;
        let outcome = update_profile_in_transaction(
            &mut transaction,
            ProfileIdentity::local_operator(),
            patch,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }
}

async fn update_profile_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
    patch: UserProfilePatch,
) -> Result<ProfileUpdateOutcome, PersistenceError> {
    let mut profile = load_profile(transaction, identity).await?;
    let previous_display_name = profile.display_name.clone();
    let previous_avatar_url = profile.avatar_image_url.clone();
    let now = Utc::now();
    let changed = profile.apply_patch(patch, now);
    if let Some(attachment_id) = avatar_attachment_id(&profile.avatar_image_url) {
        authorize_profile_avatar(transaction, identity.user_id, attachment_id, now).await?;
    }
    if !changed {
        return Ok(ProfileUpdateOutcome {
            profile,
            events: Vec::new(),
        });
    }
    sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE user_id = ?")
        .bind(serde_json::to_string(&profile)?)
        .bind(identity.user_id)
        .execute(&mut **transaction)
        .await?;
    if profile.avatar_image_url != previous_avatar_url {
        replace_profile_avatar(
            transaction,
            identity.user_id,
            &previous_avatar_url,
            &profile.avatar_image_url,
        )
        .await?;
    }
    let events = if profile.display_name != previous_display_name
        || profile.avatar_image_url != previous_avatar_url
    {
        project_profile_into_rooms(transaction, identity, &profile).await?
    } else {
        Vec::new()
    };
    Ok(ProfileUpdateOutcome { profile, events })
}

async fn load_profile(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
) -> Result<UserProfile, PersistenceError> {
    load_profile_for_identity(transaction, identity.user_id, identity.participant_id).await
}

pub(crate) async fn load_local_operator_profile(
    connection: &mut SqliteConnection,
) -> Result<UserProfile, PersistenceError> {
    let row =
        sqlx::query("SELECT participant_id, profile_json FROM user_profiles WHERE user_id = ?")
            .bind(LOCAL_OPERATOR_USER_ID)
            .fetch_optional(&mut *connection)
            .await?
            .ok_or_else(|| rejected("user_profile_missing", "Local user profile was not found."))?;
    if row.get::<String, _>("participant_id") != LOCAL_OPERATOR_PARTICIPANT_ID {
        return Err(rejected(
            "profile_authority_mismatch",
            "Local user profile does not own the operator participant.",
        ));
    }
    let profile: UserProfile = serde_json::from_str(row.get::<String, _>("profile_json").as_str())?;
    if profile.revision < 1 {
        return Err(rejected(
            "invalid_state",
            "Stored user profile revision is invalid.",
        ));
    }
    Ok(profile)
}

pub(crate) async fn load_profile_for_identity(
    transaction: &mut Transaction<'_, Sqlite>,
    user_id: &str,
    participant_id: &str,
) -> Result<UserProfile, PersistenceError> {
    let row =
        sqlx::query("SELECT participant_id, profile_json FROM user_profiles WHERE user_id = ?")
            .bind(user_id)
            .fetch_optional(&mut **transaction)
            .await?
            .ok_or_else(|| {
                rejected(
                    "user_profile_missing",
                    "Authenticated user profile was not found.",
                )
            })?;
    decode_bound_profile(
        row.get::<String, _>("participant_id").as_str(),
        participant_id,
        row.get::<String, _>("profile_json").as_str(),
    )
}

pub(crate) fn decode_bound_profile(
    stored_participant_id: &str,
    expected_participant_id: &str,
    profile_json: &str,
) -> Result<UserProfile, PersistenceError> {
    if stored_participant_id != expected_participant_id {
        return Err(rejected(
            "profile_authority_mismatch",
            "Authenticated user profile does not own this participant.",
        ));
    }
    let profile: UserProfile = serde_json::from_str(profile_json)?;
    if profile.revision < 1 {
        return Err(rejected(
            "invalid_state",
            "Stored user profile revision is invalid.",
        ));
    }
    Ok(profile)
}

pub(crate) async fn project_profile_into_rooms(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
    profile: &UserProfile,
) -> Result<Vec<RoomEvent>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT participants.room_id, participants.participant_json, rooms.room_json FROM participants JOIN rooms ON rooms.room_id = participants.room_id WHERE participants.participant_id = ? ORDER BY participants.room_id",
    )
    .bind(identity.participant_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut events = Vec::new();
    for row in rows {
        let room_id = row.get::<String, _>("room_id");
        let room: Room = serde_json::from_str(row.get::<String, _>("room_json").as_str())?;
        let mut participant: Participant =
            serde_json::from_str(row.get::<String, _>("participant_json").as_str())?;
        if room.room_id != room_id || participant.room_id != room_id {
            return Err(rejected(
                "invalid_state",
                "Stored room profile projection is invalid.",
            ));
        }
        if room.status != RoomStatus::Active
            || participant.status != ParticipantStatus::Joined
            || participant.participant_type != "human"
            || (participant.display_name == profile.display_name
                && participant.avatar_image_url == profile.avatar_image_url)
        {
            continue;
        }
        participant.display_name.clone_from(&profile.display_name);
        participant
            .avatar_image_url
            .clone_from(&profile.avatar_image_url);
        participant.updated_at = profile.updated_at;
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
        )
        .bind(serde_json::to_string(&participant)?)
        .bind(&room_id)
        .bind(identity.participant_id)
        .execute(&mut **transaction)
        .await?;
        let event = participant_updated_event(transaction, identity, profile, room_id).await?;
        sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(&event.room_id)
            .bind(event.seq)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut **transaction)
            .await?;
        events.push(event);
    }
    Ok(events)
}

async fn participant_updated_event(
    transaction: &mut Transaction<'_, Sqlite>,
    identity: ProfileIdentity<'_>,
    profile: &UserProfile,
    room_id: String,
) -> Result<RoomEvent, PersistenceError> {
    let seq = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(&room_id)
    .fetch_one(&mut **transaction)
    .await?;
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq,
        created_at: profile.updated_at,
        room_id,
        event_type: "participant_updated".to_owned(),
        actor: Actor {
            participant_id: identity.participant_id.to_owned(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(identity.participant_id.to_owned()),
        participant_type: Some("human".to_owned()),
        actor_id: Some(identity.participant_id.to_owned()),
        actor_type: Some("human".to_owned()),
        display_name: Some(profile.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([
            (
                "avatar_image_url".to_owned(),
                json!(profile.avatar_image_url),
            ),
            ("profile_revision".to_owned(), json!(profile.revision)),
        ]),
    })
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, Participant,
        ParticipantRole, ParticipantStatus, Room, RoomSettings, RoomStatus, UserProfilePatch,
    };
    use chrono::Utc;
    use sqlx::Row as _;

    use crate::{PersistenceError, SqliteStore};

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
        let url = format!(
            "sqlite:file:{}?mode=memory&cache=shared",
            uuid::Uuid::new_v4()
        );
        let store = SqliteStore::open(&url)
            .await
            .unwrap_or_else(|error| panic!("open profile fixture: {error}"));
        store
            .bootstrap_local_authority("e91430a8-e9ad-4a8a-a4ff-ebee75fc1dcc", "SeiNel")
            .await
            .unwrap_or_else(|error| panic!("bootstrap profile identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000008",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create profile room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "SeiNel".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: true,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal)
    }

    #[tokio::test]
    async fn one_profile_revision_projects_to_humans_without_crossing_room_or_agent_authority() {
        let (store, principal) = fixture().await;
        let (agent, ended_membership) =
            insert_secondary_profile_boundaries(&store, &principal).await;

        let outcome = store
            .update_user_profile(
                &principal,
                UserProfilePatch {
                    display_name: Some("Canonical Human".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("update profile: {error}"));
        assert_eq!(outcome.profile.revision, 2);
        assert_eq!(outcome.events.len(), 2);
        assert_eq!(outcome.events[0].event_type, "participant_updated");
        assert_eq!(outcome.events[0].extra["profile_revision"], 2);

        let first = store
            .participant("general", &principal.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read first membership: {error}"));
        let second = store
            .participant("second", &principal.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read second membership: {error}"));
        assert_eq!(first.display_name, "Canonical Human");
        assert_eq!(first.role, ParticipantRole::Human);
        assert!(!first.muted);
        assert_eq!(first.status, ParticipantStatus::Joined);
        assert_eq!(second.display_name, "Canonical Human");
        assert_eq!(second.role, ParticipantRole::Director);
        assert!(second.muted);
        assert_eq!(second.owner_id, "room-owner");
        let unchanged_agent = store
            .participant("general", &agent.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read agent profile: {error}"));
        assert_eq!(unchanged_agent, agent);
        let unchanged_ended = store
            .participant("ended", &principal.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read ended profile projection: {error}"));
        assert_eq!(unchanged_ended, ended_membership);

        let retry = store
            .update_user_profile(
                &principal,
                UserProfilePatch {
                    display_name: Some("Canonical Human".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await
            .unwrap_or_else(|error| panic!("retry profile: {error}"));
        assert_eq!(retry.profile.revision, 2);
        assert!(retry.events.is_empty());
        assert_eq!(retry.profile.updated_at, outcome.profile.updated_at);
    }

    #[tokio::test]
    async fn failed_projection_event_rolls_back_profile_and_membership() {
        let (store, principal) = fixture().await;
        let before = store
            .user_profile(&principal)
            .await
            .unwrap_or_else(|error| panic!("read original profile: {error}"));
        sqlx::query(
            "CREATE TRIGGER reject_profile_event BEFORE INSERT ON room_events WHEN json_extract(NEW.event_json, '$.type') = 'participant_updated' BEGIN SELECT RAISE(ABORT, 'injected profile event failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("install rollback trigger: {error}"));

        assert!(matches!(
            store
                .update_user_profile(
                    &principal,
                    UserProfilePatch {
                        display_name: Some("Must Roll Back".to_owned()),
                        ..UserProfilePatch::default()
                    },
                )
                .await,
            Err(PersistenceError::Database(_))
        ));
        let after = store
            .user_profile(&principal)
            .await
            .unwrap_or_else(|error| panic!("read rolled-back profile: {error}"));
        let membership = store
            .participant("general", &principal.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read rolled-back membership: {error}"));
        assert_eq!(after, before);
        assert_eq!(membership.display_name, before.display_name);
        let event_count = sqlx::query("SELECT COUNT(*) AS count FROM room_events")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count rolled-back events: {error}"))
            .get::<i64, _>("count");
        assert_eq!(event_count, 1);
    }

    async fn insert_secondary_profile_boundaries(
        store: &SqliteStore,
        principal: &AuthenticatedPrincipal,
    ) -> (Participant, Participant) {
        let now = Utc::now();
        let second_room = Room::new("second".to_owned(), "Second".to_owned(), now);
        sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)")
            .bind(&second_room.room_id)
            .bind(
                serde_json::to_string(&second_room)
                    .unwrap_or_else(|error| panic!("encode room: {error}")),
            )
            .bind(
                serde_json::to_string(&RoomSettings::defaults("Second"))
                    .unwrap_or_else(|error| panic!("encode settings: {error}")),
            )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("insert second room: {error}"));
        let mut ended_room = Room::new("ended".to_owned(), "Ended".to_owned(), now);
        ended_room.status = RoomStatus::Closed;
        sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, ?, ?)")
            .bind(&ended_room.room_id)
            .bind(
                serde_json::to_string(&ended_room)
                    .unwrap_or_else(|error| panic!("encode ended room: {error}")),
            )
            .bind(
                serde_json::to_string(&RoomSettings::defaults("Ended"))
                    .unwrap_or_else(|error| panic!("encode ended settings: {error}")),
            )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("insert ended room: {error}"));
        let second_membership = Participant {
            room_id: "second".to_owned(),
            participant_id: principal.participant_id.clone(),
            display_name: "stale-name".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: ParticipantRole::Director,
            owner_id: "room-owner".to_owned(),
            muted: true,
            created_at: now,
            updated_at: now,
        };
        let agent = Participant {
            room_id: "general".to_owned(),
            participant_id: "agent-owned-profile".to_owned(),
            display_name: "Independent Agent".to_owned(),
            avatar_image_url: "/agent/avatar.png".to_owned(),
            participant_type: "agent".to_owned(),
            status: ParticipantStatus::Joined,
            role: ParticipantRole::Agent,
            owner_id: principal.participant_id.clone(),
            muted: true,
            created_at: now,
            updated_at: now,
        };
        let ended_membership = Participant {
            room_id: "ended".to_owned(),
            participant_id: principal.participant_id.clone(),
            display_name: "historical-name".to_owned(),
            avatar_image_url: "/historical/avatar.png".to_owned(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Left,
            role: ParticipantRole::Human,
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        for participant in [&second_membership, &agent, &ended_membership] {
            sqlx::query(
                "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
            )
            .bind(&participant.room_id)
            .bind(&participant.participant_id)
            .bind(
                serde_json::to_string(participant)
                    .unwrap_or_else(|error| panic!("encode participant: {error}")),
            )
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("insert participant: {error}"));
        }
        (agent, ended_membership)
    }
}
