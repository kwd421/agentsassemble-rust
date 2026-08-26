use agentsassemble_domain::{
    InviteScope, LOCAL_OPERATOR_USER_ID, Participant, ParticipantRole, ParticipantStatus, Room,
    RoomStatus, UserProfile,
};
use chrono::{DateTime, TimeZone, Utc};

use crate::{
    HumanInviteCredentialEvidence, HumanInvitePreflight, HumanInvitePreflightRejection,
    HumanInvitePreflightRequest, PersistenceError, SqliteStore,
};

const GUEST_USER_ID: &str = "preflight-user";
const GUEST_PARTICIPANT_ID: &str = "preflight-person";
const CURRENT_SIGNED: [u8; 32] = [0xA1; 32];
const CURRENT_JOIN: [u8; 32] = [0xB1; 32];
const DEVICE: [u8; 32] = [0xD1; 32];

#[tokio::test]
async fn preflight_requires_profile_without_writing() {
    let store = fixture().await;
    insert_invite(&store, CURRENT_SIGNED, CURRENT_JOIN, 5, 0).await;
    let before = total_changes(&store).await;

    let decision = store
        .preflight_human_invite(&request(join_evidence(), None, None, micros(2_000_000)))
        .await
        .unwrap_or_else(|error| panic!("preflight unknown browser: {error}"));

    assert!(matches!(
        decision,
        HumanInvitePreflight::ProfileRequired(context)
            if context.room_id == "general"
                && context.room_label == "General"
                && context.invite_scope == InviteScope::ReadWrite
    ));
    assert_eq!(total_changes(&store).await, before);
}

#[tokio::test]
async fn known_device_uses_profile_ssot_and_reports_joined_membership() {
    let store = fixture().await;
    insert_invite(&store, CURRENT_SIGNED, CURRENT_JOIN, 5, 0).await;
    add_guest_profile(&store).await;
    add_device_credential(&store).await;

    let known = store
        .preflight_human_invite(&request(
            join_evidence(),
            None,
            Some(DEVICE),
            micros(2_000_000),
        ))
        .await
        .unwrap_or_else(|error| panic!("preflight known user: {error}"));
    assert!(matches!(
        known,
        HumanInvitePreflight::KnownUser { person, .. }
            if person.participant_id == GUEST_PARTICIPANT_ID
                && person.display_name == "Profile Guest"
                && !person.operator
    ));

    add_guest_participant(&store, "Stale room projection").await;
    let member = store
        .preflight_human_invite(&request(
            join_evidence(),
            None,
            Some(DEVICE),
            micros(2_000_000),
        ))
        .await
        .unwrap_or_else(|error| panic!("preflight existing member: {error}"));
    assert!(matches!(
        member,
        HumanInvitePreflight::ExistingMember { person, .. }
            if person.display_name == "Profile Guest"
    ));
}

#[tokio::test]
async fn live_same_room_session_precedes_device_and_expiry_is_read_only() {
    let store = fixture().await;
    insert_invite(&store, CURRENT_SIGNED, CURRENT_JOIN, 5, 0).await;
    let session_invite = insert_invite(&store, [0xA2; 32], [0xB2; 32], 1, 1).await;
    add_guest_profile(&store).await;
    add_guest_participant(&store, "Profile Guest").await;
    insert_session(&store, &session_invite, [0x51; 32]).await;

    let existing = store
        .preflight_human_invite(&request(
            join_evidence(),
            Some([0x51; 32]),
            None,
            micros(2_000_000),
        ))
        .await
        .unwrap_or_else(|error| panic!("preflight existing session: {error}"));
    assert!(matches!(
        existing,
        HumanInvitePreflight::ExistingSession { person, .. }
            if person.participant_id == GUEST_PARTICIPANT_ID
    ));

    let before = total_changes(&store).await;
    let expired = store
        .preflight_human_invite(&request(
            join_evidence(),
            Some([0x51; 32]),
            None,
            micros(4_000_000),
        ))
        .await
        .unwrap_or_else(|error| panic!("preflight expired session: {error}"));
    assert!(matches!(expired, HumanInvitePreflight::ProfileRequired(_)));
    assert_eq!(total_changes(&store).await, before);
    assert_eq!(stored_session_state(&store).await, "active");
}

#[tokio::test]
async fn signed_binding_and_current_invite_gates_fail_closed() {
    let store = fixture().await;
    insert_invite(&store, CURRENT_SIGNED, CURRENT_JOIN, 5, 0).await;
    let mut mismatched = signed_evidence();
    if let HumanInviteCredentialEvidence::Signed { display_name, .. } = &mut mismatched {
        *display_name = "Cross-bound name".to_owned();
    }
    assert!(matches!(
        store
            .preflight_human_invite(&request(mismatched, None, None, micros(2_000_000),))
            .await,
        Err(PersistenceError::InvalidHumanInvite)
    ));

    let expired = store
        .preflight_human_invite(&request(signed_evidence(), None, None, micros(5_000_000)))
        .await
        .unwrap_or_else(|error| panic!("preflight expired invite: {error}"));
    assert_eq!(
        expired,
        HumanInvitePreflight::Rejected(HumanInvitePreflightRejection::InviteExpired)
    );

    sqlx::query("UPDATE room_invites SET revoked = 1")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("revoke fixture invite: {error}"));
    assert_eq!(
        store
            .preflight_human_invite(&request(join_evidence(), None, None, micros(2_000_000),))
            .await
            .unwrap_or_else(|error| panic!("preflight revoked invite: {error}")),
        HumanInvitePreflight::Rejected(HumanInvitePreflightRejection::InviteRevoked)
    );

    sqlx::query("UPDATE room_invites SET revoked = 0, use_count = 5")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("exhaust fixture invite: {error}"));
    assert_eq!(
        store
            .preflight_human_invite(&request(join_evidence(), None, None, micros(2_000_000),))
            .await
            .unwrap_or_else(|error| panic!("preflight exhausted invite: {error}")),
        HumanInvitePreflight::Rejected(HumanInvitePreflightRejection::InviteUseLimitReached)
    );

    sqlx::query("UPDATE room_invites SET use_count = 0")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("restore fixture invite: {error}"));
    let room_json =
        sqlx::query_scalar::<_, String>("SELECT room_json FROM rooms WHERE room_id = 'general'")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read fixture room: {error}"));
    let mut room: Room = serde_json::from_str(&room_json)
        .unwrap_or_else(|error| panic!("decode fixture room: {error}"));
    room.status = RoomStatus::Closed;
    sqlx::query("UPDATE rooms SET room_json = ? WHERE room_id = 'general'")
        .bind(serde_json::to_string(&room).unwrap_or_else(|error| panic!("encode room: {error}")))
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("close fixture room: {error}"));
    assert_eq!(
        store
            .preflight_human_invite(&request(join_evidence(), None, None, micros(2_000_000),))
            .await
            .unwrap_or_else(|error| panic!("preflight closed room: {error}")),
        HumanInvitePreflight::Rejected(HumanInvitePreflightRejection::RoomUnavailable)
    );
}

fn request(
    credential: HumanInviteCredentialEvidence,
    session_fingerprint: Option<[u8; 32]>,
    browser_credential_fingerprint: Option<[u8; 32]>,
    now: DateTime<Utc>,
) -> HumanInvitePreflightRequest {
    HumanInvitePreflightRequest {
        credential,
        session_fingerprint,
        browser_credential_fingerprint,
        now,
    }
}

fn join_evidence() -> HumanInviteCredentialEvidence {
    HumanInviteCredentialEvidence::JoinCode {
        fingerprint: CURRENT_JOIN,
    }
}

fn signed_evidence() -> HumanInviteCredentialEvidence {
    HumanInviteCredentialEvidence::Signed {
        fingerprint: CURRENT_SIGNED,
        room_id: "general".to_owned(),
        base_participant_id: "invite-guest".to_owned(),
        display_name: "Invite Guest".to_owned(),
        invite_scope: InviteScope::ReadWrite,
        issued_at: micros(1_000_000),
        expires_at: micros(5_000_000),
    }
}

async fn fixture() -> SqliteStore {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("e5f63872-a170-4e34-98af-55940ff4a91a", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap authority: {error}"));
    store
        .create_room_for_local_operator(
            "15ebaf41-12b9-4b30-94d1-d62435b30fba",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
    store
}

async fn insert_invite(
    store: &SqliteStore,
    signed: [u8; 32],
    join: [u8; 32],
    max_uses: i64,
    use_count: i64,
) -> String {
    let invite_id = hex::encode(&signed[..8]);
    sqlx::query(
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'invite-guest', 'Invite Guest', 'read_write', ?, ?, 5000000, 0, ?, 1000000)",
    )
    .bind(&invite_id)
    .bind(signed.as_slice())
    .bind(join.as_slice())
    .bind(max_uses)
    .bind(use_count)
    .bind(LOCAL_OPERATOR_USER_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert invite: {error}"));
    invite_id
}

async fn add_guest_profile(store: &SqliteStore) {
    let profile = UserProfile::for_local_identity("Profile Guest", micros(1_100_000))
        .unwrap_or_else(|| panic!("valid guest profile"));
    sqlx::query(
        "INSERT INTO user_profiles(user_id, participant_id, profile_json) VALUES (?, ?, ?)",
    )
    .bind(GUEST_USER_ID)
    .bind(GUEST_PARTICIPANT_ID)
    .bind(serde_json::to_string(&profile).unwrap_or_else(|error| panic!("encode profile: {error}")))
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert guest profile: {error}"));
}

async fn add_device_credential(store: &SqliteStore) {
    sqlx::query(
        "INSERT INTO human_device_credentials(credential_fingerprint, user_id, created_at) VALUES (?, ?, 1200000)",
    )
    .bind(DEVICE.as_slice())
    .bind(GUEST_USER_ID)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert device credential: {error}"));
}

async fn add_guest_participant(store: &SqliteStore, display_name: &str) {
    let participant = Participant {
        room_id: "general".to_owned(),
        participant_id: GUEST_PARTICIPANT_ID.to_owned(),
        display_name: display_name.to_owned(),
        avatar_image_url: String::new(),
        participant_type: "human".to_owned(),
        status: ParticipantStatus::Joined,
        role: ParticipantRole::Human,
        owner_id: String::new(),
        muted: false,
        created_at: micros(1_200_000),
        updated_at: micros(1_200_000),
    };
    sqlx::query(
        "INSERT INTO participants(room_id, participant_id, participant_json) VALUES ('general', ?, ?)",
    )
    .bind(GUEST_PARTICIPANT_ID)
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode participant: {error}")),
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert guest participant: {error}"));
}

async fn insert_session(store: &SqliteStore, invite_id: &str, fingerprint: [u8; 32]) {
    sqlx::query(
        "INSERT INTO human_room_sessions(admission_key, key_kind, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, reusable_identity_fingerprint, result_json, admitted_at, expires_at, state) VALUES (?, 'one_use', 'aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa', ?, ?, ?, 'general', ?, ?, 'browser', 'read_write', ?, NULL, '{}', 1500000, 4000000, 'active')",
    )
    .bind([0x11; 32].as_slice())
    .bind(invite_id)
    .bind([0x22; 32].as_slice())
    .bind(fingerprint.as_slice())
    .bind(GUEST_USER_ID)
    .bind(GUEST_PARTICIPANT_ID)
    .bind([0x33; 32].as_slice())
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert session: {error}"));
}

async fn total_changes(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT total_changes()")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read total changes: {error}"))
}

async fn stored_session_state(store: &SqliteStore) -> String {
    sqlx::query_scalar("SELECT state FROM human_room_sessions")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read session state: {error}"))
}

fn micros(value: i64) -> DateTime<Utc> {
    Utc.timestamp_micros(value)
        .single()
        .unwrap_or_else(|| panic!("valid timestamp"))
}
