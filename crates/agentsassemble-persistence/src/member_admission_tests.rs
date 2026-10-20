use crate::{
    HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission, PreparedHumanAdmission,
    SqliteStore,
    human_admission_store::tests::{admitted, fixture, insert_invite, prepared_with_avatar},
};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn member(join: u8, browser: u8, name: &str) -> PreparedHumanAdmission {
    member_with_profile(join, browser, name, "", "")
}

fn member_with_profile(
    join: u8,
    browser: u8,
    name: &str,
    request_name: &str,
    avatar_image_url: &str,
) -> PreparedHumanAdmission {
    member_request(
        join,
        browser,
        &uuid::Uuid::new_v4().to_string(),
        name,
        request_name,
        avatar_image_url,
    )
}

fn member_request(
    join: u8,
    browser: u8,
    request_id: &str,
    name: &str,
    request_name: &str,
    avatar_image_url: &str,
) -> PreparedHumanAdmission {
    prepared_with_avatar(
        [join; 32],
        [browser; 32],
        request_id,
        request_name,
        avatar_image_url,
    )
    .with_member(MemberAdmission {
        secure: None,
        projection_id: "projection-test".into(),
        issuer: "https://central.example".into(),
        person_id: "person-1".into(),
        display_name: name.into(),
        registration_epoch: "epoch".into(),
        challenge_fingerprint: Sha256::digest(uuid::Uuid::new_v4().as_bytes()).into(),
        challenge_expires_at: Utc::now() + Duration::minutes(5),
    })
}

const AVATAR_ONE: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==";
const AVATAR_TWO: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGMQ0bD5DwACRAF4aig0hQAAAABJRU5ErkJggg==";

#[derive(Debug, PartialEq)]
struct JoinState {
    profile: agentsassemble_domain::UserProfile,
    participant: agentsassemble_domain::Participant,
    avatar_rows: Vec<(String, String)>,
    updated_events: i64,
}

async fn join_state(
    store: &SqliteStore,
    participant_id: &str,
) -> Result<JoinState, Box<dyn std::error::Error>> {
    let user_id = sqlx::query_scalar::<_, String>(
        "SELECT user_id FROM user_profiles WHERE participant_id = ?",
    )
    .bind(participant_id)
    .fetch_one(&store.pool)
    .await?;
    let profile: agentsassemble_domain::UserProfile = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT profile_json FROM user_profiles WHERE user_id = ?",
        )
        .bind(&user_id)
        .fetch_one(&store.pool)
        .await?,
    )?;
    let participant: agentsassemble_domain::Participant = serde_json::from_str(
        &sqlx::query_scalar::<_, String>(
            "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
        )
        .bind(participant_id)
        .fetch_one(&store.pool)
        .await?,
    )?;
    let avatar_rows = sqlx::query_as::<_, (String, String)>(
        "SELECT attachment_id, state FROM profile_avatar_assets WHERE owner_user_id = ? ORDER BY attachment_id",
    )
    .bind(&user_id)
    .fetch_all(&store.pool)
    .await?;
    let updated_events = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.type') = 'participant_updated'",
    )
    .fetch_one(&store.pool)
    .await?;
    Ok(JoinState {
        profile,
        participant,
        avatar_rows,
        updated_events,
    })
}

#[tokio::test]
async fn member_devices_retry_canonical_result_without_profile_overwrite_or_device_credentials()
-> TestResult {
    for uses in [1, 10] {
        let (store, now) = fixture().await;
        store.set_registration_epoch(Some("epoch")).await?;
        insert_invite(&store, [1; 32], [2; 32], "guest", uses, now).await;
        let first_request = member(2, 3, "First snapshot");
        let first = admitted(store.admit_human(&first_request, now).await?);
        let exact = admitted(store.admit_human(&first_request, now).await?);
        assert!(exact.deduplicated());
        let expected_bearer = first.session_bearer() == exact.session_bearer();
        assert!(expected_bearer);
        let fingerprint: [u8; 32] = Sha256::digest(first.session_bearer().as_bytes()).into();
        let auth = store.authorize_human_session(&fingerprint).await?;
        assert_eq!(auth.principal().display_name, "First snapshot");
        let replay = admitted(store.admit_human(&member(2, 4, "New name"), now).await?);
        assert!(!replay.deduplicated());
        assert!(exact.events().is_empty());
        assert_eq!(replay.events().len(), 1);
        assert_eq!(replay.events()[0].event_type, "participant_joined");
        assert_eq!(first.result().agent_id, replay.result().agent_id);
        let expected_bearer = first.session_bearer() != replay.session_bearer();
        assert!(expected_bearer);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
                .fetch_one(&store.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_device_credentials")
                .fetch_one(&store.pool)
                .await?,
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
                .fetch_one(&store.pool)
                .await?,
            1
        );
        assert!(
            store
                .revalidate_human_session_authorization(&auth)
                .await
                .is_ok()
        );
        let fingerprint: [u8; 32] = Sha256::digest(replay.session_bearer().as_bytes()).into();
        store.authorize_human_session(&fingerprint).await?;
        // Ordinary browser replay must never accept a bound user or member result.
        let mut tx = store.pool.begin().await?;
        assert!(
            crate::central_identity_bindings::require_unbound_user(
                &mut tx,
                &auth.principal().principal_id
            )
            .await
            .is_err()
        );
        tx.rollback().await?;
        // Removing exact provenance cannot turn a member session into guest authority.
        sqlx::query("UPDATE member_admissions SET session_key = ?")
            .bind([0_u8; 32].as_slice())
            .execute(&store.pool)
            .await?;
        assert!(store.authorize_human_session(&fingerprint).await.is_err());
    }
    Ok(())
}

#[tokio::test]
async fn member_join_profile_updates_the_reentry_result_and_room_participant() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let first = admitted(
        store
            .admit_human(&member(2, 3, "Central Name"), now)
            .await?,
    );
    assert_eq!(first.result().display_name, "Central Name");
    assert!(first.result().avatar_image_url.is_empty());

    let request = member_with_profile(2, 3, "Ignored central", "Room Name", "");
    let next = store.admit_human(&request, now).await?;
    let next = admitted(next);
    assert_eq!(next.result().display_name, "Room Name");
    assert_eq!(next.result().agent_id, first.result().agent_id);
    assert!(
        next.events()
            .iter()
            .any(|event| event.event_type == "participant_updated")
    );
    let participant = store
        .participant("general", &next.result().agent_id)
        .await?;
    assert_eq!(participant.display_name, "Room Name");
    let exact = admitted(store.admit_human(&request, now).await?);
    assert!(exact.deduplicated());
    assert_eq!(exact.result().display_name, "Room Name");
    Ok(())
}

#[tokio::test]
async fn capacity_rejected_rejoin_keeps_join_profile_avatar_and_participant() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 200, now).await;
    let first = admitted(
        store
            .admit_human(
                &member_with_profile(2, 3, "Central Name", "", AVATAR_ONE),
                now,
            )
            .await?,
    );
    let participant_id = first.result().agent_id.clone();
    assert!(!first.result().avatar_image_url.is_empty());
    for index in 0..111 {
        let mut request = member(2, 3, "Other member");
        request.member.as_mut().ok_or("member")?.person_id = format!("other-{index}");
        admitted(store.admit_human(&request, now).await?);
    }
    let before = join_state(&store, &participant_id).await?;

    let decision = store
        .admit_human(
            &member_with_profile(2, 4, "Central Name", "Changed", AVATAR_TWO),
            now,
        )
        .await?;
    assert!(matches!(
        decision,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::CapacityReached)
    ));
    assert_eq!(join_state(&store, &participant_id).await?, before);
    assert_eq!(before.updated_events, 0);
    Ok(())
}

#[tokio::test]
async fn conflict_rejected_rejoin_keeps_join_profile_avatar_and_participant() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let first = admitted(
        store
            .admit_human(
                &member_with_profile(2, 3, "Central Name", "", AVATAR_ONE),
                now,
            )
            .await?,
    );
    let participant_id = first.result().agent_id.clone();
    insert_invite(&store, [5; 32], [6; 32], "other", 10, now).await;
    sqlx::query(
        "UPDATE room_invites SET invite_scope = 'read_only' WHERE base_participant_id = 'other'",
    )
    .execute(&store.pool)
    .await?;
    let before = join_state(&store, &participant_id).await?;

    let decision = store
        .admit_human(
            &member_with_profile(6, 4, "Central Name", "Changed", AVATAR_TWO),
            now,
        )
        .await?;
    assert!(matches!(
        decision,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::IdempotencyConflict)
    ));
    assert_eq!(join_state(&store, &participant_id).await?, before);
    assert_eq!(before.updated_events, 0);
    Ok(())
}

#[tokio::test]
async fn unavailable_session_rejected_rejoin_keeps_join_profile_avatar_and_participant()
-> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let request_id = uuid::Uuid::new_v4().to_string();
    let first = admitted(
        store
            .admit_human(
                &member_request(2, 3, &request_id, "Central Name", "", AVATAR_ONE),
                now,
            )
            .await?,
    );
    let participant_id = first.result().agent_id.clone();
    sqlx::query("UPDATE human_room_sessions SET state = 'ended'")
        .execute(&store.pool)
        .await?;
    let before = join_state(&store, &participant_id).await?;

    let decision = store
        .admit_human(
            &member_request(2, 3, &request_id, "Central Name", "Changed", AVATAR_TWO),
            now,
        )
        .await?;
    assert!(matches!(
        decision,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
    ));
    assert_eq!(join_state(&store, &participant_id).await?, before);
    assert_eq!(before.updated_events, 0);
    Ok(())
}

#[tokio::test]
async fn admitted_rejoin_replaces_join_profile_avatar_and_participant() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let first = admitted(
        store
            .admit_human(
                &member_with_profile(2, 3, "Central Name", "", AVATAR_ONE),
                now,
            )
            .await?,
    );
    let participant_id = first.result().agent_id.clone();
    let before = join_state(&store, &participant_id).await?;
    assert_eq!(before.avatar_rows.len(), 1);

    let next = admitted(
        store
            .admit_human(
                &member_with_profile(2, 4, "Central Name", "Room Name", AVATAR_TWO),
                now,
            )
            .await?,
    );
    assert_eq!(next.result().display_name, "Room Name");
    assert_ne!(
        next.result().avatar_image_url,
        first.result().avatar_image_url
    );
    assert!(
        next.events()
            .iter()
            .any(|event| event.event_type == "participant_updated")
    );
    let after = join_state(&store, &participant_id).await?;
    assert_eq!(after.profile.display_name, "Room Name");
    assert_eq!(
        after.profile.avatar_image_url,
        next.result().avatar_image_url
    );
    assert_eq!(after.participant.display_name, "Room Name");
    assert_eq!(
        after.participant.avatar_image_url,
        next.result().avatar_image_url
    );
    assert_eq!(after.avatar_rows.len(), 1);
    assert_eq!(after.avatar_rows[0].1, "current");
    assert_ne!(after.avatar_rows[0].0, before.avatar_rows[0].0);
    assert!(after.updated_events >= 1);
    Ok(())
}

#[tokio::test]
async fn member_other_scope_and_terminal_participants_do_not_consume_or_create_sessions()
-> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest-one", 10, now).await;
    insert_invite(&store, [5; 32], [6; 32], "guest-two", 10, now).await;
    let first = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    sqlx::query("UPDATE room_invites SET invite_scope = 'read_only' WHERE base_participant_id = 'guest-two'")
        .execute(&store.pool).await?;
    assert!(matches!(
        store.admit_human(&member(6, 4, "Member"), now).await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::IdempotencyConflict)
    ));
    for status in ["left", "kicked"] {
        sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', ?) WHERE participant_id = ?")
            .bind(status).bind(&first.result().agent_id).execute(&store.pool).await?;
        for join in [2, 6] {
            assert!(matches!(
                store.admit_human(&member(join, 4, "Member"), now).await?,
                HumanAdmissionDecision::Rejected(HumanAdmissionRejection::MemberMembershipEnded)
            ));
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT SUM(use_count) FROM room_invites")
                .fetch_one(&store.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
                .fetch_one(&store.pool)
                .await?,
            1
        );
    }
    Ok(())
}

#[tokio::test]
async fn member_nonjoined_states_are_not_misreported_as_left_or_kicked() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let first = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    for status in ["exported", "detached"] {
        sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', ?) WHERE participant_id = ?")
            .bind(status).bind(&first.result().agent_id).execute(&store.pool).await?;
        assert!(matches!(
            store.admit_human(&member(2, 4, "Member"), now).await?,
            HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
        ));
    }
    Ok(())
}

#[tokio::test]
async fn member_two_device_race_reads_committed_winner() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for browser in [3, 4] {
        let store = store.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.admit_human(&member(2, browser, "Member"), now).await
        }));
    }
    let second = admitted(tasks.pop().ok_or("task")?.await??);
    let first = admitted(tasks.pop().ok_or("task")?.await??);
    assert!(!first.deduplicated() && !second.deduplicated());
    assert_eq!(first.result().agent_id, second.result().agent_id);
    let expected_bearer = first.session_bearer() != second.session_bearer();
    assert!(expected_bearer);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM human_room_sessions WHERE state = 'active'"
        )
        .fetch_one(&store.pool)
        .await?,
        2
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    Ok(())
}

#[tokio::test]
async fn member_expiry_epoch_lifecycle_and_atomic_rollback() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let mut expired = member(2, 3, "Member");
    expired
        .member
        .as_mut()
        .ok_or("member")?
        .challenge_expires_at = now;
    assert!(matches!(
        store.admit_human(&expired, now).await?,
        HumanAdmissionDecision::Rejected(_)
    ));
    store.set_registration_epoch(Some("changed")).await?;
    assert!(matches!(
        store.admit_human(&member(2, 3, "Member"), now).await?,
        HumanAdmissionDecision::Rejected(_)
    ));
    store.set_registration_epoch(Some("epoch")).await?;
    sqlx::query("CREATE TRIGGER fail_member_session BEFORE INSERT ON human_room_sessions BEGIN SELECT RAISE(ABORT, 'test failure'); END")
        .execute(&store.pool).await?;
    assert!(
        store
            .admit_human(&member(2, 3, "Member"), now)
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM member_admissions")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn member_rechecks_invite_and_room_after_redemption() -> TestResult {
    for change in [
        "UPDATE room_invites SET revoked = 1",
        "UPDATE room_invites SET use_count = max_uses",
        "UPDATE room_invites SET expires_at = created_at + 1",
        "UPDATE rooms SET room_json = json_set(room_json, '$.status', 'archived')",
    ] {
        let (store, now) = fixture().await;
        store.set_registration_epoch(Some("epoch")).await?;
        insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
        let request = member(2, 3, "Member");
        sqlx::query(change).execute(&store.pool).await?;
        assert!(matches!(
            store.admit_human(&request, now).await?,
            HumanAdmissionDecision::Rejected(_)
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
                .fetch_one(&store.pool)
                .await?,
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
                .fetch_one(&store.pool)
                .await?,
            0
        );
    }
    Ok(())
}

#[tokio::test]
async fn member_new_invite_reenters_after_original_revocation_or_expiry() -> TestResult {
    for change in [
        "UPDATE room_invites SET revoked = 1 WHERE base_participant_id = 'guest-one'",
        "UPDATE room_invites SET expires_at = created_at + 1 WHERE base_participant_id = 'guest-one'",
    ] {
        let (store, now) = fixture().await;
        store.set_registration_epoch(Some("epoch")).await?;
        insert_invite(&store, [1; 32], [2; 32], "guest-one", 10, now).await;
        insert_invite(&store, [5; 32], [6; 32], "guest-two", 10, now).await;
        let first = admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
        let users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM user_profiles")
            .fetch_one(&store.pool)
            .await?;
        sqlx::query(change).execute(&store.pool).await?;
        let replay = admitted(
            store
                .admit_human(&member(6, 4, "Other browser"), now)
                .await?,
        );
        assert!(!replay.deduplicated());
        assert_eq!(first.result().agent_id, replay.result().agent_id);
        let expected_bearer = first.session_bearer() != replay.session_bearer();
        assert!(expected_bearer);
        let fingerprint: [u8; 32] = Sha256::digest(replay.session_bearer().as_bytes()).into();
        let auth = store.authorize_human_session(&fingerprint).await?;
        assert_eq!(auth.principal().display_name, "Hihi");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_profiles")
                .fetch_one(&store.pool)
                .await?,
            users
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT use_count FROM room_invites WHERE base_participant_id = 'guest-two'"
            )
            .fetch_one(&store.pool)
            .await?,
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM member_admissions")
                .fetch_one(&store.pool)
                .await?,
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_device_credentials")
                .fetch_one(&store.pool)
                .await?,
            0
        );
        for status in ["left", "kicked"] {
            sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', ?) WHERE participant_id = ?")
                .bind(status).bind(&first.result().agent_id).execute(&store.pool).await?;
            assert!(matches!(
                store.admit_human(&member(6, 5, "Hihi"), now).await?,
                HumanAdmissionDecision::Rejected(HumanAdmissionRejection::MemberMembershipEnded)
            ));
        }
    }
    Ok(())
}

#[tokio::test]
async fn member_reenters_after_session_expiry_or_end_and_retries_exactly() -> TestResult {
    for browser in [3, 4] {
        for change in [
            "UPDATE human_room_sessions SET state = 'ended'",
            "UPDATE human_room_sessions SET admitted_at = admitted_at - 7200000000, expires_at = expires_at - 7200000000",
        ] {
            let (store, now) = fixture().await;
            store.set_registration_epoch(Some("epoch")).await?;
            insert_invite(&store, [1; 32], [2; 32], "original", 10, now).await;
            insert_invite(&store, [5; 32], [6; 32], "new", 10, now).await;
            let first = admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
            sqlx::query(change).execute(&store.pool).await?;
            let request = member(6, browser, "Ignored");
            let next = admitted(store.admit_human(&request, now).await?);
            assert_eq!(first.result().agent_id, next.result().agent_id);
            assert_eq!(next.result().display_name, "Hihi");
            let expected_bearer = first.session_bearer() != next.session_bearer();
            assert!(expected_bearer);
            let fingerprint: [u8; 32] = Sha256::digest(next.session_bearer().as_bytes()).into();
            store.authorize_human_session(&fingerprint).await?;
            let retry = admitted(store.admit_human(&request, now).await?);
            assert!(retry.deduplicated());
            assert_eq!(retry.result(), next.result());
            let expected_bearer = retry.session_bearer() == next.session_bearer();
            assert!(expected_bearer);
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(DISTINCT user_id) FROM human_room_sessions"
                )
                .fetch_one(&store.pool)
                .await?,
                1
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(DISTINCT member_admission_id) FROM human_room_sessions"
                )
                .fetch_one(&store.pool)
                .await?,
                1
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT use_count FROM room_invites WHERE base_participant_id = 'new'"
                )
                .fetch_one(&store.pool)
                .await?,
                0
            );
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
                    .fetch_one(&store.pool)
                    .await?,
                2
            );
            sqlx::query("UPDATE human_room_sessions SET browser_credential_fingerprint = ? WHERE state = 'active'")
                .bind([9_u8; 32].as_slice()).execute(&store.pool).await?;
            assert!(store.authorize_human_session(&fingerprint).await.is_err());
        }
    }
    Ok(())
}

#[tokio::test]
async fn member_same_request_race_issues_once() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
    let request = std::sync::Arc::new(member(2, 4, "Hihi"));
    let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let mut tasks = Vec::new();
    for _ in 0..2 {
        let (store, request, barrier) = (store.clone(), request.clone(), barrier.clone());
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            store.admit_human(&request, now).await
        }));
    }
    let first = admitted(tasks.pop().ok_or("task")?.await??);
    let second = admitted(tasks.pop().ok_or("task")?.await??);
    assert_ne!(first.deduplicated(), second.deduplicated());
    assert_eq!(first.result(), second.result());
    let expected_bearer = first.session_bearer() == second.session_bearer();
    assert!(expected_bearer);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await?,
        2
    );
    Ok(())
}

#[tokio::test]
async fn member_failed_reentry_rolls_back_session_replacement() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let first = admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
    sqlx::query("CREATE TRIGGER fail_reentry BEFORE INSERT ON human_room_sessions BEGIN SELECT RAISE(ABORT, 'test failure'); END")
        .execute(&store.pool).await?;
    assert!(store.admit_human(&member(2, 3, "Hihi"), now).await.is_err());
    let fingerprint: [u8; 32] = Sha256::digest(first.session_bearer().as_bytes()).into();
    store.authorize_human_session(&fingerprint).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    Ok(())
}

#[path = "member_session_tests.rs"]
mod bounded;

#[tokio::test]
async fn join_and_connect_restore_expired_live_roster_with_canonical_event() -> TestResult {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
    let host = AuthenticatedPrincipal {
        principal_id: "operator-local-user".into(),
        participant_id: "operator-local".into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    for connect in [false, true] {
        let (store, now) = fixture().await;
        store.set_registration_epoch(Some("epoch")).await?;
        insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
        let first = admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
        sqlx::query("UPDATE human_room_sessions SET admitted_at = ?, expires_at = ?")
            .bind((now - Duration::seconds(2)).timestamp_micros())
            .bind((now - Duration::seconds(1)).timestamp_micros())
            .execute(&store.pool)
            .await?;
        let authority = crate::RoomMutationAuthority::TrustedPrincipal(&host);
        let before = store.snapshot_for(authority, 0, 200).await?;
        // Account-bound membership outlives presence: an expired session keeps
        // the member listed (offline section) until leave/kick.
        assert!(
            before
                .participants
                .iter()
                .any(|p| p.participant_id == first.result().agent_id)
        );
        let request = member(2, 4, "Ignored changed name");
        let decision = if connect {
            store
                .select_member_connect_room(
                    request.member.as_ref().ok_or("member")?,
                    "general",
                    &[4; 32],
                    &request.request_id().to_string(),
                    "client",
                    now,
                )
                .await?
        } else {
            store.admit_human(&request, now).await?
        };
        let admitted = admitted(decision);
        let after = store.snapshot_for(authority, 0, 200).await?;
        let participant = after
            .participants
            .iter()
            .find(|p| p.participant_id == first.result().agent_id)
            .ok_or("restored roster")?;
        assert_eq!(admitted.events().len(), 1);
        let event = &admitted.events()[0];
        assert_eq!(event.event_type, "participant_joined");
        assert_eq!(
            event.extra["participant"],
            serde_json::to_value(participant)?
        );
        assert!(event.seq > before.last_seq);
        assert!(after.events.iter().any(|stored| stored == event));
    }
    Ok(())
}

#[tokio::test]
async fn kicked_account_member_leaves_live_roster() -> TestResult {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};
    let host = AuthenticatedPrincipal {
        principal_id: "operator-local-user".into(),
        participant_id: "operator-local".into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let first = admitted(store.admit_human(&member(2, 3, "Hihi"), now).await?);
    let authority = crate::RoomMutationAuthority::TrustedPrincipal(&host);
    assert!(
        store
            .snapshot_for(authority, 0, 200)
            .await?
            .participants
            .iter()
            .any(|p| p.participant_id == first.result().agent_id)
    );
    sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', 'kicked') WHERE participant_id = ?")
        .bind(&first.result().agent_id)
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .snapshot_for(authority, 0, 200)
            .await?
            .participants
            .iter()
            .all(|p| p.participant_id != first.result().agent_id)
    );
    Ok(())
}

#[tokio::test]
async fn secure_member_bearer_and_retry_cannot_escape_or_revive_channel() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "secure-member", 10, now).await;
    let secure = crate::SecureSessionBinding {
        client_key_fingerprint: [7; 32],
        channel_id: "a".repeat(43),
    };
    let mut request = member(2, 3, "Member");
    request.member.as_mut().ok_or("missing member")?.secure = Some(secure.clone());
    let first = admitted(store.admit_human(&request, now).await?);
    let fingerprint: [u8; 32] = Sha256::digest(first.session_bearer().as_bytes()).into();
    store
        .require_secure_human_transport(&fingerprint, Some(&secure))
        .await?;
    assert!(
        store
            .require_secure_human_transport(&fingerprint, None)
            .await
            .is_err()
    );
    let exact = admitted(store.admit_human(&request, now).await?);
    assert_eq!(first.session_bearer(), exact.session_bearer());
    request
        .member
        .as_mut()
        .ok_or("missing member")?
        .secure
        .as_mut()
        .ok_or("missing channel")?
        .channel_id = "b".repeat(43);
    assert!(store.admit_human(&request, now).await.is_err());
    store.disconnect_secure_channel(&secure.channel_id).await?;
    assert!(store.authorize_human_session(&fingerprint).await.is_err());
    request.member.as_mut().ok_or("missing member")?.secure = Some(secure);
    assert!(matches!(
        store.admit_human(&request, now).await?,
        HumanAdmissionDecision::Rejected(_)
    ));
    Ok(())
}
