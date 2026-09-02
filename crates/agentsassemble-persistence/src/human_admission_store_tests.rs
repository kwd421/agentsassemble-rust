use agentsassemble_domain::{
    LOCAL_OPERATOR_USER_ID, Participant, ParticipantRole, ParticipantStatus,
};
use chrono::{DateTime, Duration, Utc};
use sqlx::Row;

use super::{SESSION_TTL, derive_session_bearer};
use crate::{
    HumanAdmissionCommit, HumanAdmissionDecision, HumanAdmissionInput, HumanAdmissionRejection,
    HumanInviteCredentialEvidence, PersistenceError, PreparedHumanAdmission, SqliteStore,
};

const SIGNED_ONE: [u8; 32] = [0x11; 32];
const JOIN_ONE: [u8; 32] = [0x22; 32];
const BROWSER: [u8; 32] = [0x33; 32];

#[test]
fn fixed_bearer_vector_stays_inside_the_persistence_owner() {
    let issued = derive_session_bearer(&[0x11; 32], &[0x22; 32]);
    assert_eq!(
        issued.bearer,
        "aas1.azzIr-3RAkGakKN9P6yud8kvdUIp5QWcLJ3m_yDTqk4"
    );
    assert_eq!(
        hex::encode(issued.fingerprint),
        "3ffadb80cbc33f4b4090207dea9413a8f50ac39a07160c0b9bd5db4521a3c81f"
    );
}

#[tokio::test]
async fn one_use_retry_precedes_current_invite_gates_and_terminal_state_wins() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared(
        JOIN_ONE,
        BROWSER,
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "Guest",
    );

    let first = admitted(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit one-use human: {error}")),
    );
    let bearer = first.session_bearer().to_owned();
    assert!(!first.deduplicated());
    assert_eq!(first.events().len(), 1);
    assert_eq!(first.result().agent_id, "one-use-guest");
    assert_eq!(first.result().invite_scope, "room");
    assert!(!first.result().stable_identity);

    let stored_result =
        sqlx::query_scalar::<_, String>("SELECT result_json FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read stored result: {error}"));
    assert!(!stored_result.contains(&bearer));
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 1);
    sqlx::query("UPDATE room_invites SET revoked = 1")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("revoke invite: {error}"));

    let retry = admitted(
        store
            .admit_human(&request, now + Duration::seconds(1))
            .await
            .unwrap_or_else(|error| panic!("retry one-use human: {error}")),
    );
    assert!(retry.deduplicated());
    assert_eq!(retry.session_bearer(), bearer);
    assert!(retry.events().is_empty());
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 1);

    let changed = prepared(
        JOIN_ONE,
        BROWSER,
        "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
        "Changed",
    );
    assert_rejected(
        &store
            .admit_human(&changed, now + Duration::seconds(2))
            .await
            .unwrap_or_else(|error| panic!("conflicting retry: {error}")),
        HumanAdmissionRejection::IdempotencyConflict,
    );
    assert_rejected(
        &store
            .admit_human(&request, now + Duration::hours(2))
            .await
            .unwrap_or_else(|error| panic!("expired retry: {error}")),
        HumanAdmissionRejection::SessionUnavailable,
    );
    assert_eq!(session_states(&store).await, vec!["ended"]);
}

#[tokio::test]
async fn competing_final_invite_use_has_one_durable_winner() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "limited-guest", 2, now).await;
    sqlx::query("UPDATE room_invites SET use_count = 1")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("prepare final invite use: {error}"));
    let first = prepared(
        JOIN_ONE,
        [0x34; 32],
        "a2a2a2a2-a2a2-42a2-82a2-a2a2a2a2a2a2",
        "First",
    );
    let second = prepared(
        JOIN_ONE,
        [0x35; 32],
        "a3a3a3a3-a3a3-43a3-83a3-a3a3a3a3a3a3",
        "Second",
    );

    let (first, second) = tokio::join!(
        store.admit_human(&first, now),
        store.admit_human(&second, now),
    );
    let decisions = [
        first.unwrap_or_else(|error| panic!("first competing admission: {error}")),
        second.unwrap_or_else(|error| panic!("second competing admission: {error}")),
    ];

    assert_eq!(
        decisions
            .iter()
            .filter(|decision| matches!(decision, HumanAdmissionDecision::Admitted(_)))
            .count(),
        1
    );
    assert_eq!(
        decisions
            .iter()
            .filter(|decision| {
                matches!(
                    decision,
                    HumanAdmissionDecision::Rejected(
                        HumanAdmissionRejection::InviteUseLimitReached
                    )
                )
            })
            .count(),
        1
    );
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 2);
    assert_eq!(count(&store, "human_room_sessions").await, 1);
}

#[tokio::test]
async fn sub_microsecond_clock_is_canonicalized_once_for_exact_retry() {
    let (store, _) = fixture().await;
    let now = DateTime::parse_from_rfc3339("2026-08-26T09:00:00.000000001Z")
        .unwrap_or_else(|error| panic!("parse controlled clock: {error}"))
        .to_utc();
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared(
        JOIN_ONE,
        BROWSER,
        "a1a1a1a1-a1a1-41a1-81a1-a1a1a1a1a1a1",
        "Guest",
    );

    let first = admitted(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit sub-microsecond request: {error}")),
    );
    assert_eq!(
        first.result().expires_at.timestamp_subsec_nanos() % 1_000,
        0
    );
    let retry = admitted(
        store
            .admit_human(
                &request,
                now + Duration::seconds(1) + Duration::nanoseconds(1),
            )
            .await
            .unwrap_or_else(|error| panic!("retry sub-microsecond request: {error}")),
    );
    assert!(retry.deduplicated());
    assert_eq!(retry.session_bearer(), first.session_bearer());
    assert_eq!(retry.result(), first.result());
}

#[tokio::test]
async fn admission_failure_rolls_back_every_product_record() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let baseline_events = count(&store, "room_events").await;
    sqlx::query(
        "CREATE TRIGGER reject_human_session BEFORE INSERT ON human_room_sessions BEGIN SELECT RAISE(ABORT, 'injected session failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("install rollback trigger: {error}"));

    let result = store
        .admit_human(
            &prepared(
                JOIN_ONE,
                BROWSER,
                "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "Guest",
            ),
            now,
        )
        .await;
    assert!(matches!(result, Err(PersistenceError::Database(_))));
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 0);
    assert_eq!(count(&store, "human_room_sessions").await, 0);
    assert_eq!(count(&store, "human_device_credentials").await, 0);
    assert_eq!(count(&store, "user_profiles").await, 1);
    assert_eq!(count(&store, "participants").await, 1);
    assert_eq!(count(&store, "room_events").await, baseline_events);
}

#[tokio::test]
async fn join_code_routing_ignores_current_gates_but_never_authorizes_admission() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared(
        JOIN_ONE,
        BROWSER,
        "aeaeaeae-aeae-4eae-8eae-aeaeaeaeaeae",
        "Guest",
    );
    sqlx::query("UPDATE room_invites SET revoked = 1, use_count = 1")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("make invite terminal: {error}"));

    assert_eq!(
        store
            .human_admission_room_id(&request)
            .await
            .unwrap_or_else(|error| panic!("resolve routing room: {error}"))
            .as_deref(),
        Some("general")
    );
    assert_rejected(
        &store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("apply admission authority: {error}")),
        HumanAdmissionRejection::InviteRevoked,
    );
}

#[tokio::test]
async fn valid_custody_never_promotes_corrupt_avatar_metadata() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared_with_avatar(
        JOIN_ONE,
        BROWSER,
        "afafafaf-afaf-4faf-8faf-afafafafafaf",
        "Guest",
        "/api/attachments/avatar_corrupt?view=1",
    );
    sqlx::query(
        "INSERT INTO prejoin_avatar_assets(attachment_id, room_id, custody_fingerprint, invite_fingerprint, filename, content_type, content, size, created_at, expires_at) VALUES ('avatar_corrupt', 'general', ?, ?, 'avatar.png', 'image/png', X'0000', 1, ?, ?)",
    )
    .bind(request.avatar_custody_fingerprint().as_slice())
    .bind(SIGNED_ONE.as_slice())
    .bind(now.to_rfc3339())
    .bind((now + Duration::hours(1)).timestamp())
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert corrupt pending avatar: {error}"));

    assert_invalid_state(&store.admit_human(&request, now).await);
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 0);
    assert_eq!(count(&store, "human_room_sessions").await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM prejoin_avatar_assets WHERE attachment_id = 'avatar_corrupt'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read corrupt prejoin avatar: {error}")),
        1
    );

    sqlx::query("UPDATE prejoin_avatar_assets SET size = 2 WHERE attachment_id = 'avatar_corrupt'")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("repair pending avatar metadata: {error}"));
    let admitted = admitted(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit repaired avatar: {error}")),
    );
    assert_eq!(
        admitted.result().avatar_image_url,
        "/api/attachments/avatar_corrupt?view=1"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM profile_avatar_assets WHERE attachment_id = 'avatar_corrupt'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read bound avatar state: {error}")),
        "current"
    );
}

#[tokio::test]
async fn corrupt_retry_authority_fails_closed_without_terminalizing_session() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared(
        JOIN_ONE,
        BROWSER,
        "abababab-abab-4bab-8bab-abababababab",
        "Guest",
    );
    let first = admitted(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit one-use human: {error}")),
    );
    let participant_id = first.result().agent_id.clone();

    let original_participant = sqlx::query_scalar::<_, String>(
        "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(&participant_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read participant: {error}"));
    let mut corrupt_participant: Participant = serde_json::from_str(&original_participant)
        .unwrap_or_else(|error| panic!("decode participant: {error}"));
    corrupt_participant.room_id = "other-room".to_owned();
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(
        serde_json::to_string(&corrupt_participant)
            .unwrap_or_else(|error| panic!("encode participant: {error}")),
    )
    .bind(&participant_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("corrupt participant: {error}"));

    assert_invalid_state(
        &store
            .admit_human(&request, now + Duration::seconds(1))
            .await,
    );
    assert_eq!(session_states(&store).await, vec!["active"]);

    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(&original_participant)
    .bind(&participant_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("restore participant: {error}"));
    let profile_json = sqlx::query_scalar::<_, String>(
        "SELECT profile_json FROM user_profiles WHERE participant_id = ?",
    )
    .bind(&participant_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read profile: {error}"));
    let mut profile: serde_json::Value = serde_json::from_str(&profile_json)
        .unwrap_or_else(|error| panic!("decode profile: {error}"));
    profile["revision"] = serde_json::json!(0);
    sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE participant_id = ?")
        .bind(profile.to_string())
        .bind(&participant_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("corrupt profile: {error}"));

    assert_invalid_state(
        &store
            .admit_human(&request, now + Duration::seconds(2))
            .await,
    );
    assert_eq!(session_states(&store).await, vec!["active"]);
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 1);
}

#[tokio::test]
async fn active_session_with_nonjoined_membership_is_not_silently_repaired() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "one-use-guest", 1, now).await;
    let request = prepared(
        JOIN_ONE,
        BROWSER,
        "bcbcbcbc-bcbc-4cbc-8cbc-bcbcbcbcbcbc",
        "Guest",
    );
    let first = admitted(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit one-use human: {error}")),
    );
    let participant_id = first.result().agent_id.clone();
    let mut participant: Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(&participant_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read participant: {error}")),
    )
    .unwrap_or_else(|error| panic!("decode participant: {error}"));
    participant.status = ParticipantStatus::Left;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(
        serde_json::to_string(&participant)
            .unwrap_or_else(|error| panic!("encode participant: {error}")),
    )
    .bind(&participant_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("make participant unavailable: {error}"));

    assert_invalid_state(
        &store
            .admit_human(&request, now + Duration::seconds(1))
            .await,
    );
    assert_eq!(session_states(&store).await, vec!["active"]);
    assert_eq!(invite_use_count(&store, SIGNED_ONE).await, 1);
}

#[tokio::test]
async fn reusable_identity_rejects_a_corrupt_profile_before_patching_it() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "unused-one", 5, now).await;
    let first = admitted(
        store
            .admit_human(
                &prepared(
                    JOIN_ONE,
                    BROWSER,
                    "acacacac-acac-4cac-8cac-acacacacacac",
                    "First",
                ),
                now,
            )
            .await
            .unwrap_or_else(|error| panic!("first reusable admission: {error}")),
    );
    let participant_id = first.result().agent_id.clone();
    let profile_json = sqlx::query_scalar::<_, String>(
        "SELECT profile_json FROM user_profiles WHERE participant_id = ?",
    )
    .bind(&participant_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read profile: {error}"));
    let mut profile: serde_json::Value = serde_json::from_str(&profile_json)
        .unwrap_or_else(|error| panic!("decode profile: {error}"));
    profile["revision"] = serde_json::json!(0);
    sqlx::query("UPDATE user_profiles SET profile_json = ? WHERE participant_id = ?")
        .bind(profile.to_string())
        .bind(&participant_id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("corrupt profile: {error}"));

    let signed_two = [0x44; 32];
    let join_two = [0x55; 32];
    insert_invite(&store, signed_two, join_two, "unused-two", 5, now).await;
    assert_invalid_state(
        &store
            .admit_human(
                &prepared(
                    join_two,
                    BROWSER,
                    "adadadad-adad-4dad-8dad-adadadadadad",
                    "Second",
                ),
                now + Duration::seconds(1),
            )
            .await,
    );
    assert_eq!(invite_use_count(&store, signed_two).await, 0);
    assert_eq!(session_states(&store).await, vec!["active"]);
}

#[tokio::test]
async fn reusable_identity_replaces_only_its_session_and_preserves_room_authority() {
    let (store, now) = fixture().await;
    insert_invite(&store, SIGNED_ONE, JOIN_ONE, "unused-one", 5, now).await;
    let first_request = prepared(
        JOIN_ONE,
        BROWSER,
        "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
        "First",
    );
    let first = admitted(
        store
            .admit_human(&first_request, now)
            .await
            .unwrap_or_else(|error| panic!("first reusable admission: {error}")),
    );
    let participant_id = first.result().agent_id.clone();
    let first_bearer = first.session_bearer().to_owned();
    assert_eq!(
        first.result().guide["session"]["rejoin"],
        "This session cannot be renewed after it expires; ask the host for a new invite link."
    );
    assert_eq!(
        first.result().guide["session"]["expires_in_seconds"].as_i64(),
        Some(SESSION_TTL.num_seconds())
    );
    set_room_authority(&store, &participant_id).await;

    let signed_two = [0x44; 32];
    let join_two = [0x55; 32];
    insert_invite(&store, signed_two, join_two, "unused-two", 5, now).await;
    let second_request = prepared(
        join_two,
        BROWSER,
        "dddddddd-dddd-4ddd-8ddd-dddddddddddd",
        "Second",
    );
    let second = admitted(
        store
            .admit_human(&second_request, now + Duration::seconds(1))
            .await
            .unwrap_or_else(|error| panic!("replacement admission: {error}")),
    );
    assert_eq!(second.result().agent_id, participant_id);
    assert_ne!(second.session_bearer(), first_bearer);
    assert_eq!(second.replaced_session_fingerprints().len(), 1);
    assert_eq!(
        second
            .events()
            .iter()
            .map(|event| event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["participant_updated"]
    );
    assert_eq!(session_states(&store).await, vec!["ended", "active"]);
    let participant = store
        .participant("general", &participant_id)
        .await
        .unwrap_or_else(|error| panic!("read replaced participant: {error}"));
    assert_eq!(participant.display_name, "Second");
    assert_eq!(participant.role, ParticipantRole::Reviewer);
    assert!(participant.muted);

    let retry = admitted(
        store
            .admit_human(
                &prepared(
                    join_two,
                    BROWSER,
                    "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                    "Second",
                ),
                now + Duration::seconds(2),
            )
            .await
            .unwrap_or_else(|error| panic!("reusable retry: {error}")),
    );
    assert!(retry.deduplicated());
    assert_eq!(retry.session_bearer(), second.session_bearer());
    assert_eq!(
        retry.result().request_id,
        "dddddddd-dddd-4ddd-8ddd-dddddddddddd"
    );
    assert_eq!(invite_use_count(&store, signed_two).await, 1);
    sqlx::query("UPDATE room_invites SET revoked = 1 WHERE signed_token_fingerprint = ?")
        .bind(signed_two.as_slice())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("revoke reusable invite: {error}"));
    assert_rejected(
        &store
            .admit_human(&second_request, now + Duration::seconds(3))
            .await
            .unwrap_or_else(|error| panic!("revoked reusable retry: {error}")),
        HumanAdmissionRejection::InviteRevoked,
    );
}

fn prepared(
    invite_fingerprint: [u8; 32],
    browser_fingerprint: [u8; 32],
    request_id: &str,
    display_name: &str,
) -> PreparedHumanAdmission {
    prepared_with_avatar(
        invite_fingerprint,
        browser_fingerprint,
        request_id,
        display_name,
        "",
    )
}

fn prepared_with_avatar(
    invite_fingerprint: [u8; 32],
    browser_fingerprint: [u8; 32],
    request_id: &str,
    display_name: &str,
    avatar_image_url: &str,
) -> PreparedHumanAdmission {
    PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: invite_fingerprint,
        },
        browser_fingerprint,
        &HumanAdmissionInput {
            request_id: request_id.to_owned(),
            meeting_id_assertion: "general".to_owned(),
            display_name: display_name.to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: "Host".to_owned(),
            client_id: "browser-client".to_owned(),
            avatar_image_url: avatar_image_url.to_owned(),
        },
    )
    .unwrap_or_else(|error| panic!("prepare admission: {error}"))
}

async fn fixture() -> (SqliteStore, DateTime<Utc>) {
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
    (store, Utc::now())
}

async fn insert_invite(
    store: &SqliteStore,
    signed: [u8; 32],
    join: [u8; 32],
    base_participant_id: &str,
    max_uses: i64,
    now: DateTime<Utc>,
) {
    sqlx::query(
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', ?, 'Invite Guest', 'read_write', ?, 0, ?, 0, ?, ?)",
    )
    .bind(hex::encode(&signed[..8]))
    .bind(signed.as_slice())
    .bind(join.as_slice())
    .bind(base_participant_id)
    .bind(max_uses)
    .bind((now + Duration::days(1)).timestamp_micros())
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind((now - Duration::minutes(1)).timestamp_micros())
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert invite: {error}"));
}

async fn set_room_authority(store: &SqliteStore, participant_id: &str) {
    let mut participant: Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(participant_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read participant authority: {error}")),
    )
    .unwrap_or_else(|error| panic!("decode participant authority: {error}"));
    participant.role = ParticipantRole::Reviewer;
    participant.muted = true;
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = 'general' AND participant_id = ?",
    )
    .bind(serde_json::to_string(&participant).unwrap_or_else(|error| panic!("encode: {error}")))
    .bind(participant_id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("update participant authority: {error}"));
}

fn admitted(decision: HumanAdmissionDecision) -> Box<HumanAdmissionCommit> {
    match decision {
        HumanAdmissionDecision::Admitted(commit) => commit,
        HumanAdmissionDecision::Rejected(_) => panic!("expected admitted decision"),
    }
}

fn assert_rejected(decision: &HumanAdmissionDecision, expected: HumanAdmissionRejection) {
    match decision {
        HumanAdmissionDecision::Rejected(actual) => assert_eq!(*actual, expected),
        HumanAdmissionDecision::Admitted(_) => panic!("expected rejected decision"),
    }
}

fn assert_invalid_state(result: &Result<HumanAdmissionDecision, PersistenceError>) {
    assert!(matches!(
        result,
        Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            ..
        })
    ));
}

async fn invite_use_count(store: &SqliteStore, signed: [u8; 32]) -> i64 {
    sqlx::query_scalar("SELECT use_count FROM room_invites WHERE signed_token_fingerprint = ?")
        .bind(signed.as_slice())
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read invite use count: {error}"))
}

async fn session_states(store: &SqliteStore) -> Vec<String> {
    sqlx::query("SELECT state FROM human_room_sessions ORDER BY admitted_at")
        .fetch_all(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read session states: {error}"))
        .into_iter()
        .map(|row| row.get("state"))
        .collect()
}

async fn count(store: &SqliteStore, table: &str) -> i64 {
    let sql = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(sqlx::AssertSqlSafe(sql))
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count {table}: {error}"))
}
