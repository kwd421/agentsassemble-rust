use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    Participant, RoomEvent, UserProfile, UserProfilePatch, avatar_attachment_id,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{Row, Sqlite, Transaction};
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
        let profile = load_profile(&mut transaction, principal).await?;
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
        let mut profile = load_profile(&mut transaction, principal).await?;
        let previous_display_name = profile.display_name.clone();
        let previous_avatar_url = profile.avatar_image_url.clone();
        let now = Utc::now();
        let changed = profile.apply_patch(patch, now);
        if let Some(attachment_id) = avatar_attachment_id(&profile.avatar_image_url) {
            authorize_profile_avatar(
                &mut transaction,
                &principal.principal_id,
                attachment_id,
                now,
            )
            .await?;
        }
        if !changed {
            transaction.commit().await?;
            return Ok(ProfileUpdateOutcome {
                profile,
                events: Vec::new(),
            });
        }
        sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE user_id = ?")
            .bind(serde_json::to_string(&profile)?)
            .bind(&principal.principal_id)
            .execute(&mut *transaction)
            .await?;
        if profile.avatar_image_url != previous_avatar_url {
            replace_profile_avatar(
                &mut transaction,
                &principal.principal_id,
                &previous_avatar_url,
                &profile.avatar_image_url,
            )
            .await?;
        }
        let events = if profile.display_name != previous_display_name
            || profile.avatar_image_url != previous_avatar_url
        {
            project_profile_into_rooms(&mut transaction, principal, &profile).await?
        } else {
            Vec::new()
        };
        transaction.commit().await?;
        Ok(ProfileUpdateOutcome { profile, events })
    }
}

pub(crate) async fn insert_initial_local_profile(
    transaction: &mut Transaction<'_, Sqlite>,
    participant: &Participant,
) -> Result<(), PersistenceError> {
    if participant.participant_id != LOCAL_OPERATOR_PARTICIPANT_ID
        || participant.participant_type != "human"
    {
        return Ok(());
    }
    let mut profile = UserProfile::defaults(participant.created_at);
    profile.display_name.clone_from(&participant.display_name);
    profile
        .avatar_image_url
        .clone_from(&participant.avatar_image_url);
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
    )
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind(LOCAL_OPERATOR_PARTICIPANT_ID)
    .bind(serde_json::to_string(&profile)?)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_profile(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
) -> Result<UserProfile, PersistenceError> {
    load_profile_for_identity(
        transaction,
        &principal.principal_id,
        &principal.participant_id,
    )
    .await
}

pub(crate) async fn load_local_operator_profile(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<UserProfile, PersistenceError> {
    load_profile_for_identity(
        transaction,
        LOCAL_OPERATOR_USER_ID,
        LOCAL_OPERATOR_PARTICIPANT_ID,
    )
    .await
}

async fn load_profile_for_identity(
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
    if row.get::<String, _>("participant_id") != participant_id {
        return Err(rejected(
            "profile_authority_mismatch",
            "Authenticated user profile does not own this participant.",
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

async fn project_profile_into_rooms(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    profile: &UserProfile,
) -> Result<Vec<RoomEvent>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT room_id, participant_json FROM participants WHERE participant_id = ? ORDER BY room_id",
    )
    .bind(&principal.participant_id)
    .fetch_all(&mut **transaction)
    .await?;
    let mut events = Vec::new();
    for row in rows {
        let room_id = row.get::<String, _>("room_id");
        let mut participant: Participant =
            serde_json::from_str(row.get::<String, _>("participant_json").as_str())?;
        if participant.participant_type != "human"
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
        .bind(&principal.participant_id)
        .execute(&mut **transaction)
        .await?;
        let event = participant_updated_event(transaction, principal, profile, room_id).await?;
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
    principal: &AuthenticatedPrincipal,
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
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(principal.participant_id.clone()),
        participant_type: Some("human".to_owned()),
        actor_id: Some(principal.participant_id.clone()),
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
        ParticipantStatus, Room, RoomSettings, UserProfilePatch,
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
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: "operator-local".to_owned(),
            display_name: "SeiNel".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        store
            .initialize_room(&room, &RoomSettings::defaults("General"), &participant)
            .await
            .unwrap_or_else(|error| panic!("initialize profile fixture: {error}"));
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
        let agent = insert_secondary_profile_boundaries(&store, &principal).await;

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
        assert_eq!(first.role, "host");
        assert!(!first.muted);
        assert_eq!(first.status, ParticipantStatus::Joined);
        assert_eq!(second.display_name, "Canonical Human");
        assert_eq!(second.role, "moderator");
        assert!(second.muted);
        assert_eq!(second.owner_id, "room-owner");
        let unchanged_agent = store
            .participant("general", &agent.participant_id)
            .await
            .unwrap_or_else(|error| panic!("read agent profile: {error}"));
        assert_eq!(unchanged_agent, agent);

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
        assert_eq!(event_count, 0);
    }

    async fn insert_secondary_profile_boundaries(
        store: &SqliteStore,
        principal: &AuthenticatedPrincipal,
    ) -> Participant {
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
        let second_membership = Participant {
            room_id: "second".to_owned(),
            participant_id: principal.participant_id.clone(),
            display_name: "stale-name".to_owned(),
            avatar_image_url: String::new(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "moderator".to_owned(),
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
            role: "agent".to_owned(),
            owner_id: principal.participant_id.clone(),
            muted: true,
            created_at: now,
            updated_at: now,
        };
        for participant in [&second_membership, &agent] {
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
        agent
    }
}
