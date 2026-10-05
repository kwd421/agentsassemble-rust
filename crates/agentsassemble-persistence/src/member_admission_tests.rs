use crate::{
    HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission, PreparedHumanAdmission,
    human_admission_store::tests::{admitted, fixture, insert_invite, prepared},
};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn member(join: u8, browser: u8, name: &str) -> PreparedHumanAdmission {
    prepared(
        [join; 32],
        [browser; 32],
        &uuid::Uuid::new_v4().to_string(),
        "ignored",
    )
    .with_member(MemberAdmission {
        issuer: "https://central.example".into(),
        person_id: "person-1".into(),
        display_name: name.into(),
        registration_epoch: "epoch".into(),
        challenge_expires_at: Utc::now() + Duration::minutes(5),
    })
}

#[tokio::test]
async fn member_devices_retry_canonical_result_without_profile_overwrite_or_device_credentials()
-> TestResult {
    for uses in [1, 10] {
        let (store, now) = fixture().await;
        store.set_registration_epoch(Some("epoch")).await?;
        insert_invite(&store, [1; 32], [2; 32], "guest", uses, now).await;
        let first = admitted(
            store
                .admit_human(&member(2, 3, "First snapshot"), now)
                .await?,
        );
        let fingerprint: [u8; 32] = Sha256::digest(first.session_bearer().as_bytes()).into();
        let auth = store.authorize_human_session(&fingerprint).await?;
        assert_eq!(auth.principal().display_name, "First snapshot");
        let replay = admitted(store.admit_human(&member(2, 4, "New name"), now).await?);
        assert!(replay.deduplicated());
        assert_eq!(first.result(), replay.result());
        assert_eq!(first.session_bearer(), replay.session_bearer());
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
        store.revalidate_human_session_authorization(&auth).await?;
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
                HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
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
    assert_ne!(first.deduplicated(), second.deduplicated());
    assert_eq!(first.result(), second.result());
    assert_eq!(first.session_bearer(), second.session_bearer());
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
        assert!(replay.deduplicated());
        assert_eq!(first.result(), replay.result());
        assert_eq!(first.session_bearer(), replay.session_bearer());
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
                HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
            ));
        }
    }
    Ok(())
}
