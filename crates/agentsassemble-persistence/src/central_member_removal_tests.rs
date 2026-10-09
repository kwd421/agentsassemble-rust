use super::*;
use crate::human_admission_store::tests::{admitted, fixture, insert_invite, prepared};
use crate::{HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission};
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;
fn secure() -> SecureSessionBinding {
    SecureSessionBinding {
        client_key_fingerprint: [9; 32],
        channel_id: "c".repeat(43),
    }
}
fn principal(epoch: &str) -> Result<MemberRemovalPrincipal, PersistenceError> {
    MemberRemovalPrincipal::verified(
        "https://central.example".into(),
        "person-1".into(),
        "r".repeat(43),
        epoch.into(),
        Utc::now() + chrono::Duration::seconds(120),
        secure(),
    )
}
fn member() -> MemberAdmission {
    MemberAdmission {
        secure: None,
        projection_id: "test-projection".into(),
        issuer: "https://central.example".into(),
        person_id: "person-1".into(),
        display_name: "Private Name".into(),
        registration_epoch: "epoch".into(),
        challenge_fingerprint: [5; 32],
        challenge_expires_at: Utc::now() + chrono::Duration::seconds(300),
    }
}

// Regression oracle: public admission/session revalidation and committed fence,
// not an in-memory removal marker. Temporarily removing each common check must
// allow exactly that forbidden path while stored work remains unchanged.
#[tokio::test]
async fn removal_commit_fences_both_existing_devices_replay_connect_and_mutation() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let a = prepared(
        [2; 32],
        [3; 32],
        &uuid::Uuid::new_v4().to_string(),
        "ignored",
    )
    .with_member(member());
    let b = prepared(
        [2; 32],
        [4; 32],
        &uuid::Uuid::new_v4().to_string(),
        "ignored",
    )
    .with_member(member());
    let first = admitted(store.admit_human(&a, now).await?);
    let second = admitted(store.admit_human(&b, now).await?);
    let fingerprint = |bearer: &str| -> [u8; 32] { Sha256::digest(bearer.as_bytes()).into() };
    let one = store
        .authorize_human_session(&fingerprint(first.session_bearer()))
        .await?;
    store
        .authorize_human_session(&fingerprint(second.session_bearer()))
        .await?;
    let key = store
        .begin_member_account_removal(&principal("epoch")?, &secure())
        .await?;
    assert_eq!(key.person_id(), "person-1");
    assert!(
        store
            .authorize_human_session(&fingerprint(first.session_bearer()))
            .await
            .is_err()
    );
    assert!(
        store
            .authorize_human_session(&fingerprint(second.session_bearer()))
            .await
            .is_err()
    );
    assert!(
        store
            .revalidate_human_session_authorization(&one)
            .await
            .is_err()
    );
    assert!(store.member_connect_rooms(&member(), now).await.is_err());
    assert!(matches!(
        store.admit_human(&a, now).await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
    ));
    assert!(
        store
            .update_user_profile(
                one.principal(),
                0,
                agentsassemble_domain::UserProfilePatch {
                    display_name: Some("Resurrected".into()),
                    ..Default::default()
                }
            )
            .await
            .is_err()
    );
    verify_retry_and_committed_member_page(
        &store,
        &key,
        [
            fingerprint(first.session_bearer()),
            fingerprint(second.session_bearer()),
        ],
    )
    .await
}

async fn verify_retry_and_committed_member_page(
    store: &SqliteStore,
    key: &MemberRemovalKey,
    expected: [[u8; 32]; 2],
) -> TestResult {
    // Repeat proof-bound entry continues the original work, never creates another binding/job.
    store
        .begin_member_account_removal(&principal("epoch")?, &secure())
        .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT phase FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        "owner_sessions"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    assert_eq!(store.local_operator_profile().await?.display_name, "Host");
    assert_eq!(
        store
            .advance_member_removal_authority(key)
            .await?
            .next_phase,
        "owner_pairings"
    );
    assert_eq!(
        store
            .advance_member_removal_authority(key)
            .await?
            .next_phase,
        "human_sessions"
    );
    let page = store.advance_member_removal_authority(key).await?;
    assert_eq!(page.next_phase, "companions");
    let actual = page
        .room_sessions
        .into_iter()
        .map(|(room, fp)| {
            assert_eq!(room, "general");
            fp
        })
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(actual, std::collections::HashSet::from(expected));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM human_room_sessions WHERE state='ended'"
        )
        .fetch_one(&store.pool)
        .await?,
        2
    );
    Ok(())
}

#[tokio::test]
async fn wrong_channel_epoch_and_failed_initial_work_leave_no_fence() -> TestResult {
    let (store, _) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    let mut other = secure();
    other.client_key_fingerprint = [8; 32];
    assert!(
        store
            .begin_member_account_removal(&principal("epoch")?, &other)
            .await
            .is_err()
    );
    assert!(
        store
            .begin_member_account_removal(&principal("other")?, &secure())
            .await
            .is_err()
    );
    sqlx::query("CREATE TRIGGER reject_removal BEFORE INSERT ON central_member_removals BEGIN SELECT RAISE(ABORT,'test work failure'); END")
        .execute(&store.pool).await?;
    assert!(
        store
            .begin_member_account_removal(&principal("epoch")?, &secure())
            .await
            .is_err()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    sqlx::query("DROP TRIGGER reject_removal")
        .execute(&store.pool)
        .await?;
    store
        .begin_member_account_removal(&principal("epoch")?, &secure())
        .await?;
    let now = Utc::now();
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let request = prepared(
        [2; 32],
        [3; 32],
        &uuid::Uuid::new_v4().to_string(),
        "ignored",
    )
    .with_member(member());
    assert!(matches!(
        store.admit_human(&request, now).await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_identity_bindings")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn v86_expansion_preserves_frozen_binding_and_room_then_rejects_unknown_work_schema()
-> TestResult {
    let (store, _) = fixture().await;
    sqlx::query("DROP TABLE central_member_removals")
        .execute(&store.pool)
        .await?;
    sqlx::query("UPDATE runtime_metadata SET value='86' WHERE key='schema_version'")
        .execute(&store.pool)
        .await?;
    upgrade(&store.pool).await?;
    crate::schema_version::validate_schema_version(&store.pool).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM rooms")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    store.set_registration_epoch(Some("epoch")).await?;
    sqlx::query("UPDATE runtime_metadata SET value='88' WHERE key='schema_version'")
        .execute(&store.pool)
        .await?;
    assert!(matches!(
        store
            .begin_member_account_removal(&principal("epoch")?, &secure())
            .await,
        Err(PersistenceError::InvalidSchemaVersion(_))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM central_member_removals")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    Ok(())
}
