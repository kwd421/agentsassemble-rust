use agentsassemble_domain::{
    ClientKind, InviteScope, LOCAL_OPERATOR_USER_ID, Participant, ParticipantStatus, UserProfile,
    UserProfilePatch,
};
use chrono::{DateTime, Duration, Utc};

use crate::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence, PersistenceError,
    PreparedHumanAdmission, SqliteStore,
};

const SIGNED: [u8; 32] = [0x41; 32];
const JOIN: [u8; 32] = [0x42; 32];
const BROWSER: [u8; 32] = [0x43; 32];

#[tokio::test]
async fn live_human_session_authority_revalidates_scope_membership_and_profile() {
    let (store, now) = admitted_fixture().await;
    let fingerprint = session_fingerprint(&store).await;

    let authority = store
        .authorize_human_session(&fingerprint)
        .await
        .unwrap_or_else(|error| panic!("authorize live human session: {error}"));
    assert_eq!(authority.session_fingerprint(), &fingerprint);
    assert!(authority.expires_at() > now);
    assert!(
        authority
            .principal()
            .principal_id
            .starts_with("u-admission-")
    );
    assert_eq!(authority.principal().participant_id, "session-guest");
    assert_eq!(authority.principal().display_name, "Session Guest");
    assert_eq!(authority.principal().room_id, "general");
    assert_eq!(authority.principal().client_kind, ClientKind::Browser);
    assert_eq!(authority.principal().invite_scope, InviteScope::ReadOnly);
    assert!(!authority.principal().is_operator);
    assert!(!authority.principal().capabilities.message_send);

    let profile = store
        .human_session_profile(&authority)
        .await
        .unwrap_or_else(|error| panic!("read session profile: {error}"));
    assert_eq!(profile.display_name, "Session Guest");
    let updated = store
        .update_human_session_profile(
            &authority,
            UserProfilePatch {
                display_name: Some("Updated Session Guest".to_owned()),
                ..UserProfilePatch::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("update session profile: {error}"));
    assert_eq!(updated.profile.display_name, "Updated Session Guest");
    assert_eq!(updated.events.len(), 1);
    assert_eq!(
        store
            .human_session_profile(&authority)
            .await
            .unwrap_or_else(|error| panic!("read changed session profile: {error}"))
            .display_name,
        "Updated Session Guest"
    );

    set_session_expiry(&store, authority.expires_at() + Duration::minutes(1)).await;
    assert_rejected_code(
        store.human_session_profile(&authority).await,
        "invalid_state",
    );
    set_session_expiry(&store, authority.expires_at()).await;

    set_participant_status(&store, ParticipantStatus::Left).await;
    assert_rejected_code(
        store.authorize_human_session(&fingerprint).await,
        "session_revoked",
    );
    assert_rejected_code(
        store.human_session_profile(&authority).await,
        "session_revoked",
    );
    assert_rejected_code(
        store
            .update_human_session_profile(
                &authority,
                UserProfilePatch {
                    custom_status: Some("must not commit".to_owned()),
                    ..UserProfilePatch::default()
                },
            )
            .await,
        "session_revoked",
    );

    set_participant_status(&store, ParticipantStatus::Joined).await;
    assert!(
        store
            .human_session_profile(&authority)
            .await
            .unwrap_or_else(|error| panic!("read profile after rejected update: {error}"))
            .custom_status
            .is_empty()
    );
    set_profile_revision(&store, &authority.principal().principal_id, 0).await;
    assert_rejected_code(
        store.authorize_human_session(&fingerprint).await,
        "invalid_state",
    );
}

async fn set_session_expiry(store: &SqliteStore, expires_at: DateTime<Utc>) {
    sqlx::query("UPDATE human_room_sessions SET expires_at = ?")
        .bind(expires_at.timestamp_micros())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("update session expiry: {error}"));
}

async fn admitted_fixture() -> (SqliteStore, DateTime<Utc>) {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open session authority fixture: {error}"));
    store
        .bootstrap_local_authority("ac96ca39-fbcd-4a22-a8f6-fc937341f2f2", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap session authority: {error}"));
    store
        .create_room_for_local_operator(
            "64994f26-b72c-4d0e-a1f1-e3803080420c",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create session authority room: {error}"));
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'session-guest', 'Session Guest', 'read_only', 1, 0, ?, 0, ?, ?)",
    )
    .bind(hex::encode(&SIGNED[..8]))
    .bind(SIGNED.as_slice())
    .bind(JOIN.as_slice())
    .bind((now + Duration::hours(2)).timestamp_micros())
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind((now - Duration::minutes(1)).timestamp_micros())
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert session authority invite: {error}"));
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode { fingerprint: JOIN },
        BROWSER,
        &HumanAdmissionInput {
            request_id: "f1f1f1f1-f1f1-41f1-81f1-f1f1f1f1f1f1".to_owned(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Session Guest".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: "Host".to_owned(),
            client_id: "session-authority-browser".to_owned(),
            avatar_image_url: String::new(),
        },
    )
    .unwrap_or_else(|error| panic!("prepare session authority admission: {error}"));
    match store
        .admit_human(&request, now)
        .await
        .unwrap_or_else(|error| panic!("admit session authority fixture: {error}"))
    {
        HumanAdmissionDecision::Admitted(_) => {}
        HumanAdmissionDecision::Rejected(_) => panic!("session authority fixture was rejected"),
    }
    (store, now)
}

async fn session_fingerprint(store: &SqliteStore) -> [u8; 32] {
    sqlx::query_scalar::<_, Vec<u8>>("SELECT session_fingerprint FROM human_room_sessions")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read session fingerprint: {error}"))
        .try_into()
        .unwrap_or_else(|value: Vec<u8>| {
            panic!("invalid session fingerprint length: {}", value.len())
        })
}

async fn set_participant_status(store: &SqliteStore, status: ParticipantStatus) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = 'session-guest'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read session participant: {error}"));
    let mut participant: Participant = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode session participant: {error}"));
    participant.status = status;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = 'session-guest'",
    )
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode session participant: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("update session participant: {error}"));
}

async fn set_profile_revision(store: &SqliteStore, user_id: &str, revision: i64) {
    let encoded =
        sqlx::query_scalar::<_, String>("SELECT profile_json FROM user_profiles WHERE user_id = ?")
            .bind(user_id)
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read session profile: {error}"));
    let mut profile: UserProfile = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode session profile: {error}"));
    profile.revision = revision;
    sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE user_id = ?")
        .bind(
            serde_json::to_string(&profile)
                .unwrap_or_else(|error| panic!("encode session profile: {error}")),
        )
        .bind(user_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("update session profile: {error}"));
}

fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected} rejection, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
}
