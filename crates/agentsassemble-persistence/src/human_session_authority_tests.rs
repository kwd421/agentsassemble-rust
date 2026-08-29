use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, Participant, ParticipantStatus, RoomRandomResult, RoomSettings,
    UserProfile, UserProfilePatch, public_settings,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{DateTime, Duration, Utc};
use serde_json::json;

use crate::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
    HumanSessionAuthorization, PersistenceError, PreparedHumanAdmission, SqliteStore,
};

const SIGNED: [u8; 32] = [0x41; 32];
const JOIN: [u8; 32] = [0x42; 32];
const BROWSER: [u8; 32] = [0x43; 32];

#[tokio::test]
async fn live_human_session_authority_revalidates_scope_membership_and_profile() {
    let (store, now) = admitted_fixture(InviteScope::ReadOnly).await;
    let fingerprint = session_fingerprint(&store).await;

    let authority = store
        .authorize_human_session(&fingerprint)
        .await
        .unwrap_or_else(|error| panic!("authorize live human session: {error}"));
    assert_live_session_identity(&authority, &fingerprint, now);

    let profile = store
        .human_session_profile(&authority)
        .await
        .unwrap_or_else(|error| panic!("read session profile: {error}"));
    assert_eq!(profile.display_name, "Session Guest");
    let updated = store
        .update_human_session_profile(
            &authority,
            profile.revision,
            UserProfilePatch {
                display_name: Some("Updated Session Guest".to_owned()),
                ..UserProfilePatch::default()
            },
        )
        .await
        .unwrap_or_else(|error| panic!("update session profile: {error}"));
    assert_eq!(updated.profile.display_name, "Updated Session Guest");
    assert_eq!(updated.events.len(), 1);
    assert_refreshed_display(&store, &authority, "Updated Session Guest").await;
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
    assert_rejected_code(
        store
            .revalidate_human_session_authorization(&authority)
            .await,
        "invalid_state",
    );
    set_session_expiry(&store, authority.expires_at()).await;

    set_participant_status(&store, ParticipantStatus::Left).await;
    assert_rejected_code(
        store.authorize_human_session(&fingerprint).await,
        "session_revoked",
    );
    assert_rejected_code(
        store
            .revalidate_human_session_authorization(&authority)
            .await,
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
                updated.profile.revision,
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

#[tokio::test]
async fn human_session_avatar_upload_requires_live_write_scope() {
    let (read_only_store, _) = admitted_fixture(InviteScope::ReadOnly).await;
    let read_only = read_only_store
        .authorize_human_session(&session_fingerprint(&read_only_store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize read-only avatar session: {error}"));
    assert_rejected_code(
        read_only_store
            .store_human_session_profile_attachment(
                &read_only,
                "ignored.png",
                "image/png",
                b"not decoded for read-only authority".to_vec(),
            )
            .await,
        "session_read_only",
    );

    let (store, _) = admitted_fixture(InviteScope::ReadWrite).await;
    let authorization = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize writable avatar session: {error}"));
    let stored = store
        .store_human_session_profile_attachment(
            &authorization,
            "guest.png",
            "image/png",
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==")
                .unwrap_or_else(|error| panic!("decode avatar fixture: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("store writable session avatar: {error}"));
    set_participant_status(&store, ParticipantStatus::Left).await;
    assert_rejected_code(
        store
            .store_human_session_profile_attachment(
                &authorization,
                "replacement.png",
                "image/png",
                STANDARD
                    .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==")
                    .unwrap_or_else(|error| panic!("decode replacement fixture: {error}")),
            )
            .await,
        "session_revoked",
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT attachment_id FROM profile_avatar_assets WHERE owner_user_id = ? AND state = 'pending'",
        )
        .bind(&authorization.principal().principal_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read retained session avatar: {error}")),
        stored.id
    );
}

#[tokio::test]
async fn human_session_message_upload_revalidates_write_scope_and_mute_state() {
    let (read_only_store, _) = admitted_fixture(InviteScope::ReadOnly).await;
    let read_only = read_only_store
        .authorize_human_session(&session_fingerprint(&read_only_store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize read-only message upload: {error}"));
    assert_rejected_code(
        read_only_store
            .store_human_session_message_attachment(
                &read_only,
                "denied.txt",
                "text/plain",
                b"denied".to_vec(),
            )
            .await,
        "permission_denied",
    );

    let (store, _) = admitted_fixture(InviteScope::ReadWrite).await;
    let authorization = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize writable message upload: {error}"));
    let stored = store
        .store_human_session_message_attachment(
            &authorization,
            "guest.txt",
            "text/plain",
            b"guest attachment".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store writable message upload: {error}"));
    set_participant_muted(&store, true).await;
    assert_rejected_code(
        store
            .store_human_session_message_attachment(
                &authorization,
                "muted.txt",
                "text/plain",
                b"muted".to_vec(),
            )
            .await,
        "muted",
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT attachment_id FROM room_message_attachments WHERE pending_owner_user_id = ?",
        )
        .bind(&authorization.principal().principal_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read retained message upload: {error}")),
        stored.id
    );
}

#[tokio::test]
async fn bound_appearance_read_revalidates_human_session_in_the_asset_snapshot() {
    let (store, _) = admitted_fixture(InviteScope::ReadOnly).await;
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize appearance manager: {error}"));
    let stored = store
        .store_pending_room_appearance_asset(
            &manager,
            "room-icon.png",
            "image/png",
            STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==")
                .unwrap_or_else(|error| panic!("decode appearance fixture: {error}")),
        )
        .await
        .unwrap_or_else(|error| panic!("store appearance fixture: {error}"));
    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("read appearance revision: {error}"))
        .settings_revision;
    store
        .execute_room_settings_update(
            &local_operator_principal(),
            "human-session-appearance-bind",
            &json!({
                "expected_revision": revision,
                "appearance": {"icon_image_url": stored.url}
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("bind appearance fixture: {error}"));
    let authorization = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize appearance reader: {error}"));

    let asset = store
        .bound_human_session_room_appearance_asset(&authorization, &stored.id)
        .await
        .unwrap_or_else(|error| panic!("read session-bound appearance: {error}"));
    assert_eq!(&asset.content[..8], b"\x89PNG\r\n\x1a\n");

    set_participant_status(&store, ParticipantStatus::Left).await;
    assert_rejected_code(
        store
            .bound_human_session_room_appearance_asset(&authorization, &stored.id)
            .await,
        "session_revoked",
    );
}

#[tokio::test]
async fn session_originated_command_units_revalidate_exact_provenance() {
    let (store, _) = admitted_fixture(InviteScope::ReadWrite).await;
    let authorization = store
        .authorize_human_session(&session_fingerprint(&store).await)
        .await
        .unwrap_or_else(|error| panic!("authorize command session: {error}"));
    let baseline_events = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count baseline events: {error}"));

    set_session_expiry(&store, authorization.expires_at() + Duration::minutes(1)).await;
    assert_rejected_code(
        store
            .execute_human_session_message_with_turn(
                &authorization,
                "revoked-message",
                "message.send",
                &json!({"content": "must not commit"}),
            )
            .await,
        "invalid_state",
    );

    set_session_expiry(&store, authorization.expires_at()).await;
    set_participant_status(&store, ParticipantStatus::Left).await;
    assert_rejected_code(
        store
            .execute_human_session_room_random_command(
                &authorization,
                "revoked-random",
                "room.random.roll",
                &json!({"notation": "1d6"}),
                &RoomRandomResult::RollDice {
                    notation: "1d6".to_owned(),
                    rolls: vec![3],
                    modifier: 0,
                    total: 3,
                },
            )
            .await,
        "session_revoked",
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count rejected command events: {error}")),
        baseline_events
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM command_results")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count rejected command results: {error}")),
        0
    );
}

fn local_operator_principal() -> AuthenticatedPrincipal {
    AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    }
}

async fn set_session_expiry(store: &SqliteStore, expires_at: DateTime<Utc>) {
    sqlx::query("UPDATE human_room_sessions SET expires_at = ?")
        .bind(expires_at.timestamp_micros())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("update session expiry: {error}"));
}

async fn assert_refreshed_display(
    store: &SqliteStore,
    authority: &crate::HumanSessionAuthorization,
    expected: &str,
) {
    let refreshed = store
        .revalidate_human_session_authorization(authority)
        .await
        .unwrap_or_else(|error| panic!("refresh changed session profile: {error}"));
    assert_eq!(refreshed.principal().display_name, expected);
}

async fn admitted_fixture(invite_scope: InviteScope) -> (SqliteStore, DateTime<Utc>) {
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
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'session-guest', 'Session Guest', ?, 1, 0, ?, 0, ?, ?)",
    )
    .bind(hex::encode(&SIGNED[..8]))
    .bind(SIGNED.as_slice())
    .bind(JOIN.as_slice())
    .bind(match invite_scope {
        InviteScope::ReadWrite => "read_write",
        InviteScope::ReadOnly => "read_only",
    })
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

fn assert_live_session_identity(
    authority: &HumanSessionAuthorization,
    fingerprint: &[u8; 32],
    now: DateTime<Utc>,
) {
    assert_eq!(authority.session_fingerprint(), fingerprint);
    assert!(authority.expires_at() > now);
    let principal = authority.principal();
    assert!(principal.principal_id.starts_with("u-admission-"));
    assert_eq!(principal.participant_id, "session-guest");
    assert_eq!(principal.display_name, "Session Guest");
    assert_eq!(principal.room_id, "general");
    assert_eq!(principal.client_kind, ClientKind::Browser);
    assert_eq!(principal.invite_scope, InviteScope::ReadOnly);
    assert!(!principal.is_operator);
    assert!(!principal.capabilities.message_send);
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

async fn set_participant_muted(store: &SqliteStore, muted: bool) {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = 'session-guest'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read session participant: {error}"));
    let mut participant: Participant = serde_json::from_str(&encoded)
        .unwrap_or_else(|error| panic!("decode session participant: {error}"));
    participant.muted = muted;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = 'session-guest'",
    )
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode session participant: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("update session participant mute state: {error}"));
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
