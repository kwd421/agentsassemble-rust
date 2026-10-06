use crate::human_admission_store::tests::{fixture, insert_invite, prepared};
use crate::{HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission};
use chrono::Duration;
use sha2::{Digest, Sha256};

type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn connect_rechecks_membership_scope_and_never_consumes_invites() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let member = MemberAdmission {
        projection_id: "id".into(),
        issuer: "https://central.example".into(),
        person_id: "person".into(),
        display_name: "name".into(),
        registration_epoch: "epoch".into(),
        challenge_fingerprint: [4; 32],
        challenge_expires_at: now + Duration::minutes(5),
    };
    let request = prepared([2; 32], [3; 32], &uuid::Uuid::new_v4().to_string(), "name")
        .with_member(member.clone());
    store.admit_human(&request, now).await?;
    sqlx::query("UPDATE room_invites SET revoked=1")
        .execute(&store.pool)
        .await?;
    let rooms = store.member_connect_rooms(&member, now).await?;
    assert_eq!(rooms.len(), 1);
    let id = uuid::Uuid::new_v4().to_string();
    let first = store
        .select_member_connect_room(&member, "general", &[5; 32], &id, "client", now)
        .await?;
    let HumanAdmissionDecision::Admitted(first) = first else {
        panic!("selected")
    };
    assert_eq!(first.result().invite_scope, "room");
    let HumanAdmissionDecision::Admitted(retry) = store
        .select_member_connect_room(&member, "general", &[5; 32], &id, "client", now)
        .await?
    else {
        panic!("retry")
    };
    let same_session = first.session_bearer() == retry.session_bearer();
    assert!(same_session);
    let fingerprint = Sha256::digest(first.session_bearer().as_bytes()).into();
    store.authorize_human_session(&fingerprint).await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM member_admissions")
            .fetch_one(&store.pool)
            .await?,
        1
    );
    sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','kicked') WHERE participant_id IN (SELECT participant_id FROM member_admissions)").execute(&store.pool).await?;
    assert!(matches!(
        store
            .select_member_connect_room(&member, "general", &[5; 32], &id, "client", now)
            .await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::MemberMembershipEnded)
    ));
    assert!(store.member_connect_rooms(&member, now).await?.is_empty());
    Ok(())
}
