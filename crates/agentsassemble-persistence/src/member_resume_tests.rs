use crate::human_admission_store::tests::{fixture, insert_invite, prepared};
use crate::{
    HumanAdmissionDecision, HumanAdmissionRejection, MemberAdmission, MemberResumeKeyIssue,
    PersistenceError, SqliteStore,
};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use sqlx::Connection as _;

type TestResult = Result<(), Box<dyn std::error::Error>>;
type TestValue<T> = Result<T, Box<dyn std::error::Error>>;

const ISSUER: &str = "https://central.example";
const PERSON: &str = "person-1";
const EPOCH: &str = "epoch";

fn member() -> MemberAdmission {
    MemberAdmission {
        secure: None,
        projection_id: "id".into(),
        issuer: ISSUER.into(),
        person_id: PERSON.into(),
        display_name: "Member".into(),
        registration_epoch: EPOCH.into(),
        challenge_fingerprint: [4; 32],
        challenge_expires_at: Utc::now() + Duration::minutes(5),
    }
}

fn public_key(seed: u8) -> [u8; 65] {
    let mut key = [0u8; 65];
    key[0] = 4;
    key[1..].copy_from_slice(&[seed; 64]);
    key
}

async fn issue(
    store: &SqliteStore,
    person: &str,
    browser: &[u8; 32],
    key: &[u8; 65],
    now: DateTime<Utc>,
) -> TestResult {
    store
        .issue_member_resume_key(MemberResumeKeyIssue {
            issuer: ISSUER,
            person_id: person,
            registration_epoch: EPOCH,
            browser_credential_fingerprint: browser,
            resume_public_key: key,
            issued_at: now,
            expires_at: now + Duration::hours(1),
        })
        .await?;
    Ok(())
}

async fn stored_key(
    store: &SqliteStore,
    browser: &[u8; 32],
    key: &[u8; 65],
) -> TestValue<crate::MemberResumeKey> {
    let found = store.member_resume_key(browser, key).await?;
    let Some(found) = found else {
        panic!("the stored resume key must exist");
    };
    Ok(found)
}

async fn direct_admission(
    store: &SqliteStore,
    browser: &[u8; 32],
    key: &[u8; 65],
    now: DateTime<Utc>,
) -> TestValue<MemberAdmission> {
    let stored = stored_key(store, browser, key).await?;
    let Some(admission) = store
        .member_resume_admission(&stored, now, Duration::seconds(300))
        .await?
    else {
        panic!("the direct admission must exist");
    };
    Ok(admission)
}

async fn admitted_member(store: &SqliteStore, now: DateTime<Utc>) -> TestResult {
    insert_invite(store, [1; 32], [2; 32], "guest", 10, now).await;
    let request = prepared(
        [2; 32],
        [3; 32],
        &uuid::Uuid::new_v4().to_string(),
        "Member",
    )
    .with_member(member());
    store.admit_human(&request, now).await?;
    Ok(())
}

#[tokio::test]
async fn resume_key_is_bound_to_the_live_epoch_and_the_browser_credential() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(7);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    let found = stored_key(&store, &[5; 32], &key).await?;
    assert_eq!(found.issuer, ISSUER);
    assert_eq!(found.person_id, PERSON);
    assert_eq!(found.registration_epoch, EPOCH);
    assert_eq!(found.resume_public_key, key);
    assert_eq!(found.expires_at, now + Duration::hours(1));
    // Another browser credential never reaches this person's key.
    assert!(store.member_resume_key(&[6; 32], &key).await?.is_none());
    // Another public key never reaches this browser credential's row.
    assert!(
        store
            .member_resume_key(&[5; 32], &public_key(8))
            .await?
            .is_none()
    );
    // A rotated registration epoch retires the row with the incarnation.
    store.set_registration_epoch(Some("next-epoch")).await?;
    assert!(store.member_resume_key(&[5; 32], &key).await?.is_none());
    Ok(())
}

#[tokio::test]
async fn one_browser_credential_replaces_its_own_key_and_the_person_keeps_eight() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let first = public_key(1);
    issue(&store, PERSON, &[1; 32], &first, now).await?;
    // The same browser credential replaces its own row instead of adding one.
    let second = public_key(2);
    issue(&store, PERSON, &[1; 32], &second, now).await?;
    assert!(store.member_resume_key(&[1; 32], &first).await?.is_none());
    assert!(store.member_resume_key(&[1; 32], &second).await?.is_some());
    for index in 0..10u8 {
        issue(
            &store,
            PERSON,
            &[index + 20; 32],
            &public_key(index + 30),
            now + Duration::seconds(i64::from(index)),
        )
        .await?;
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM member_resume_keys")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(count, 8, "one person cannot accumulate unbounded resume keys");
    // The oldest row is the one that goes.
    assert!(store.member_resume_key(&[1; 32], &second).await?.is_none());
    assert!(
        store
            .member_resume_key(&[29; 32], &public_key(39))
            .await?
            .is_some()
    );
    Ok(())
}

#[tokio::test]
async fn direct_resume_keeps_the_stored_projection_and_acked_revision() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    admitted_member(&store, now).await?;
    let key = public_key(11);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    // The stored anchor already holds the central projection id.
    sqlx::query("UPDATE member_projection_outbox SET acked_revision=5")
        .execute(&store.pool)
        .await?;
    let admission = direct_admission(&store, &[5; 32], &key, now).await?;
    assert_eq!(admission.projection_id, "id");
    let rooms = store.member_connect_rooms(&admission, now).await?;
    assert_eq!(rooms.len(), 1);
    let request_id = uuid::Uuid::new_v4().to_string();
    let decision = store
        .select_member_connect_room(&admission, "general", &[5; 32], &request_id, "client", now)
        .await?;
    assert!(matches!(decision, HumanAdmissionDecision::Admitted(_)));
    let row: (String, i64) =
        sqlx::query_as("SELECT projection_id, acked_revision FROM member_projection_outbox")
            .fetch_one(&store.pool)
            .await?;
    assert_eq!(row.0, "id");
    assert_eq!(
        row.1, 5,
        "a direct list/select must not reset the central acknowledgement"
    );
    // A projection id that is not the stored one is never used on this path.
    let mut foreign = admission.clone();
    foreign.projection_id = "other".into();
    store.member_connect_rooms(&foreign, now).await?;
    let reset: i64 = sqlx::query_scalar("SELECT acked_revision FROM member_projection_outbox")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(reset, 0, "a foreign projection id rewrites the anchor");
    Ok(())
}

#[tokio::test]
async fn a_kick_or_a_leave_refuses_the_direct_select_while_another_room_still_lists() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    admitted_member(&store, now).await?;
    // A second joined room on the same key stays listable.
    store
        .create_room_for_local_operator(
            &uuid::Uuid::new_v4().to_string(),
            "second",
            "Second",
        )
        .await?;
    sqlx::query("INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'second', 'second-guest', 'Second Guest', 'read_write', 1, 0, ?, 0, ?, ?)")
        .bind(hex::encode(&[6_u8; 32][..8]))
        .bind([6_u8; 32].as_slice())
        .bind([7_u8; 32].as_slice())
        .bind((now + Duration::days(1)).timestamp_micros())
        .bind(agentsassemble_domain::LOCAL_OPERATOR_USER_ID)
        .bind((now - Duration::minutes(1)).timestamp_micros())
        .execute(&store.pool)
        .await?;
    let second = crate::PreparedHumanAdmission::prepare(
        crate::HumanInviteCredentialEvidence::JoinCode {
            fingerprint: [7_u8; 32],
        },
        [4_u8; 32],
        &crate::HumanAdmissionInput {
            request_id: uuid::Uuid::new_v4().to_string(),
            meeting_id_assertion: String::new(),
            display_name: "Member".into(),
            participant_type: "human".into(),
            owner_display_name: String::new(),
            client_id: "second-client".into(),
            avatar_image_url: String::new(),
        },
    )?
    .with_member(member());
    store.admit_human(&second, now).await?;
    let key = public_key(13);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    let admission = direct_admission(&store, &[5; 32], &key, now).await?;
    assert_eq!(store.member_connect_rooms(&admission, now).await?.len(), 2);
    sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','kicked') WHERE room_id='general'")
        .execute(&store.pool)
        .await?;
    let request_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        store
            .select_member_connect_room(&admission, "general", &[5; 32], &request_id, "client", now)
            .await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::MemberMembershipEnded)
    ));
    // The kicked room leaves the list; the other joined room stays.
    let rooms = store.member_connect_rooms(&admission, now).await?;
    assert_eq!(rooms.len(), 1);
    assert_eq!(rooms[0].room_id, "second");
    // A leave refuses the direct select exactly like a kick.
    sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','left') WHERE room_id='second'")
        .execute(&store.pool)
        .await?;
    let request_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        store
            .select_member_connect_room(&admission, "second", &[5; 32], &request_id, "client", now)
            .await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::MemberMembershipEnded)
    ));
    // The left room leaves the list as well.
    assert!(
        store.member_connect_rooms(&admission, now).await?.is_empty(),
        "a left member keeps no joined room"
    );
    Ok(())
}

#[tokio::test]
async fn account_deletion_and_epoch_rotation_delete_the_resume_row() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(17);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    // A different person keeps its own row.
    let other = public_key(19);
    issue(&store, "person-2", &[6; 32], &other, now).await?;
    let binding = crate::SecureSessionBinding {
        client_key_fingerprint: [9; 32],
        channel_id: "c".repeat(43),
    };
    store
        .begin_member_account_removal(
            &crate::central_member_removal::MemberRemovalPrincipal::verified(
                ISSUER.into(),
                PERSON.into(),
                "r".repeat(43),
                EPOCH.into(),
                Utc::now() + Duration::seconds(120),
                binding.clone(),
            )?,
            &binding,
        )
        .await?;
    assert!(
        store.member_resume_key(&[5; 32], &key).await?.is_none(),
        "the removed person's resume key must die with the fence"
    );
    assert!(store.member_resume_key(&[6; 32], &other).await?.is_some());
    // The fence refuses the direct path for that person.
    let mut tx = store.pool.begin().await?;
    assert!(
        !crate::central_member_removal::person_is_live(&mut tx, ISSUER, PERSON).await?
    );
    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn a_rotated_incarnation_drops_every_resume_row() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(23);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    sqlx::query("INSERT INTO runtime_metadata(key,value) VALUES ('named_tunnel','{\"registration_epoch\":\"epoch\"}') ON CONFLICT(key) DO UPDATE SET value=excluded.value")
        .execute(&store.pool)
        .await?;
    store.set_registration_epoch(Some("next-epoch")).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM member_resume_keys")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(count, 0, "an epoch change retires every resume key");
    let tunnel: Option<String> =
        sqlx::query_scalar("SELECT value FROM runtime_metadata WHERE key='named_tunnel'")
            .fetch_optional(&store.pool)
            .await?;
    assert!(tunnel.is_none(), "the epoch change drops the named tunnel token");
    Ok(())
}

#[tokio::test]
async fn an_expired_key_never_opens_the_direct_path() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    admitted_member(&store, now).await?;
    let key = public_key(29);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    let stored = stored_key(&store, &[5; 32], &key).await?;
    assert!(
        store
            .member_resume_admission(&stored, now + Duration::hours(2), Duration::seconds(300))
            .await?
            .is_none(),
        "an expired key must not open the direct path"
    );
    // A successful direct attempt never moves the expiry forward.
    let admission = direct_admission(&store, &[5; 32], &key, now).await?;
    store.member_connect_rooms(&admission, now).await?;
    let after = stored_key(&store, &[5; 32], &key).await?;
    assert_eq!(after.expires_at, stored.expires_at);
    Ok(())
}

#[tokio::test]
async fn resume_keys_are_ephemeral_and_never_survive_a_restore() -> TestResult {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("source.sqlite")).await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Host")
        .await?;
    let now = Utc::now();
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(31);
    issue(&store, PERSON, &[5; 32], &key, now).await?;
    let snapshot = directory.path().join("snapshot.sqlite");
    let payload = directory.path().join("payload.sqlite");
    store
        .export_backup_payload(&crate::ServerOwnerAuthority::LocalOperator, &snapshot, &payload)
        .await?;
    let rebuilt = directory.path().join("rebuilt.sqlite");
    crate::backup_database::reconstruct_payload(
        &payload,
        &rebuilt,
        crate::schema_version::CURRENT_SCHEMA_VERSION,
    )
    .await?;
    let mut restored = crate::backup_database::open_input(&rebuilt).await?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM member_resume_keys")
        .fetch_one(&mut restored)
        .await?;
    assert_eq!(count, 0, "a resume key must never survive a restore");
    restored.close().await?;
    let live: i64 = sqlx::query_scalar("SELECT count(*) FROM member_resume_keys")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(live, 1, "the export must not change the live host");
    Ok(())
}

#[tokio::test]
async fn a_malformed_resume_request_is_refused_without_storing_a_row() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(37);
    assert!(
        store
            .issue_member_resume_key(MemberResumeKeyIssue {
                issuer: ISSUER,
                person_id: PERSON,
                registration_epoch: EPOCH,
                browser_credential_fingerprint: &[5; 32],
                resume_public_key: &key,
                issued_at: now,
                expires_at: now,
            })
            .await
            .is_err(),
        "an expiry that is not in the future is refused"
    );
    assert!(
        store
            .issue_member_resume_key(MemberResumeKeyIssue {
                issuer: "",
                person_id: PERSON,
                registration_epoch: EPOCH,
                browser_credential_fingerprint: &[5; 32],
                resume_public_key: &key,
                issued_at: now,
                expires_at: now + Duration::hours(1),
            })
            .await
            .is_err()
    );
    assert!(
        store
            .issue_member_resume_key(MemberResumeKeyIssue {
                issuer: ISSUER,
                person_id: PERSON,
                registration_epoch: "",
                browser_credential_fingerprint: &[5; 32],
                resume_public_key: &key,
                issued_at: now,
                expires_at: now + Duration::hours(1),
            })
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM member_resume_keys")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(count, 0);
    let _: PersistenceError = crate::account_identity::rejected("member_resume_invalid", "x");
    Ok(())
}

#[tokio::test]
async fn the_stored_fingerprint_is_the_only_browser_credential_that_opens_the_key() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some(EPOCH)).await?;
    let key = public_key(41);
    let fingerprint = Sha256::digest(b"browser-credential");
    let mut browser = [0u8; 32];
    browser.copy_from_slice(&fingerprint);
    issue(&store, PERSON, &browser, &key, now).await?;
    assert!(store.member_resume_key(&browser, &key).await?.is_some());
    let mut other = browser;
    other[0] ^= 1;
    assert!(store.member_resume_key(&other, &key).await?.is_none());
    Ok(())
}
