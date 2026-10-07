use super::*;
use crate::human_admission_store::tests::{fixture, insert_invite, prepared};
use chrono::Duration;
use sha2::Digest;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn projection_revision_replacement_late_ack_and_epoch_are_exact() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let request = prepared([2; 32], [3; 32], &uuid::Uuid::new_v4().to_string(), "name")
        .with_member(crate::MemberAdmission {
            secure: None,
            projection_id: "first".into(),
            issuer: "https://central.example".into(),
            person_id: "person".into(),
            display_name: "name".into(),
            registration_epoch: "epoch".into(),
            challenge_fingerprint: [4; 32],
            challenge_expires_at: now + Duration::minutes(5),
        });
    assert!(matches!(
        store.admit_human(&request, now).await?,
        crate::HumanAdmissionDecision::Admitted(_)
    ));
    let old = store
        .take_member_projection_batch(now.timestamp(), 20)
        .await?;
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].state, "active");
    let mut tx = store.pool.begin().await?;
    let mut replacement = request.member.clone().ok_or("member")?;
    replacement.projection_id = "replacement".into();
    anchor(&mut tx, &old[0].binding_id, &replacement, false).await?;
    tx.commit().await?;
    store
        .finish_member_projection_batch(&old, false, now.timestamp())
        .await?;
    let new = store
        .take_member_projection_batch(now.timestamp() + 60, 20)
        .await?;
    assert_eq!(new.len(), 1);
    assert_eq!(new[0].revision, old[0].revision);
    assert_eq!(new[0].projection_id, "replacement");
    store
        .finish_member_projection_batch(&new, true, now.timestamp() + 60)
        .await?;
    assert!(
        store
            .take_member_projection_batch(now.timestamp() + 120, 20)
            .await?
            .is_empty()
    );
    let mut tx = store.pool.begin().await?;
    sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.status','left') WHERE participant_id IN (SELECT participant_id FROM member_admissions)").execute(&mut *tx).await?;
    room_changed(&mut tx, "general").await?;
    tx.commit().await?;
    let removed = store
        .take_member_projection_batch(now.timestamp() + 120, 20)
        .await?;
    assert_eq!(removed[0].state, "removed");
    assert_eq!(removed[0].revision, new[0].revision + 1);
    store.set_registration_epoch(Some("other")).await?;
    store.set_registration_epoch(Some("epoch")).await?;
    assert!(
        store
            .take_member_projection_batch(now.timestamp() + 10000, 20)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test]
async fn retry_schedule_and_daily_budget_are_durable() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let request = prepared([2; 32], [3; 32], &uuid::Uuid::new_v4().to_string(), "name")
        .with_member(crate::MemberAdmission {
            secure: None,
            projection_id: "id".into(),
            issuer: "https://central.example".into(),
            person_id: "person".into(),
            display_name: "name".into(),
            registration_epoch: "epoch".into(),
            challenge_fingerprint: [4; 32],
            challenge_expires_at: now + Duration::minutes(5),
        });
    store.admit_human(&request, now).await?;
    let start = 86400 * 20000;
    assert_eq!(
        store.take_member_projection_batch(start, 20).await?.len(),
        1
    );
    assert!(
        store
            .take_member_projection_batch(start + 59, 20)
            .await?
            .is_empty()
    );
    assert_eq!(
        store
            .take_member_projection_batch(start + 60, 20)
            .await?
            .len(),
        1
    );
    let deadline: i64 = sqlx::query_scalar("SELECT next_attempt_at FROM member_projection_sender")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(deadline, start + 180);
    // A successful report permits more work after one minute, but never request 49.
    for i in 2..48 {
        sqlx::query("UPDATE member_projection_sender SET next_attempt_at=0")
            .execute(&store.pool)
            .await?;
        assert_eq!(
            store
                .take_member_projection_batch(start + i * 60, 20)
                .await?
                .len(),
            1
        );
    }
    sqlx::query("UPDATE member_projection_sender SET next_attempt_at=0")
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .take_member_projection_batch(start + 4000, 20)
            .await?
            .is_empty()
    );
    assert_eq!(
        store
            .take_member_projection_batch(start + 86400, 20)
            .await?
            .len(),
        1
    );
    Ok(())
}

#[tokio::test]
async fn leaving_one_of_two_rooms_keeps_server_visible() -> TestResult {
    use crate::human_admission_store::tests::admitted;
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 10, now).await;
    let member = crate::MemberAdmission {
        secure: None,
        projection_id: "id".into(),
        issuer: "https://central.example".into(),
        person_id: "person".into(),
        display_name: "name".into(),
        registration_epoch: "epoch".into(),
        challenge_fingerprint: [4; 32],
        challenge_expires_at: now + Duration::minutes(5),
    };
    let first = admitted(
        store
            .admit_human(
                &prepared([2; 32], [3; 32], &uuid::Uuid::new_v4().to_string(), "name")
                    .with_member(member),
                now,
            )
            .await?,
    );
    let mut tx = store.pool.begin().await?;
    sqlx::query("INSERT INTO rooms(room_id,room_json,settings_json) SELECT 'second',json_set(room_json,'$.room_id','second','$.room_uid',?),settings_json FROM rooms WHERE room_id='general'")
        .bind(uuid::Uuid::new_v4().to_string()).execute(&mut *tx).await?;
    sqlx::query("INSERT INTO participants(room_id,participant_id,participant_json) SELECT 'second',participant_id,json_set(participant_json,'$.room_id','second') FROM participants WHERE room_id='general'").execute(&mut *tx).await?;
    sqlx::query("INSERT INTO member_admissions SELECT binding_id,'second-invite','second-admission','second',invite_scope,input_hash,user_id,participant_id,session_key,result_json,replay_floor FROM member_admissions WHERE room_id='general'").execute(&mut *tx).await?;
    room_changed(&mut tx, "second").await?;
    tx.commit().await?;
    let fingerprint = sha2::Sha256::digest(first.session_bearer().as_bytes()).into();
    let auth = store.authorize_human_session(&fingerprint).await?;
    store
        .execute_participant_leave(auth.principal(), "leave-first", &serde_json::json!({}))
        .await?;
    let batch = store
        .take_member_projection_batch(now.timestamp(), 20)
        .await?;
    assert_eq!(batch.len(), 1);
    assert_eq!(batch[0].state, "active");
    assert_eq!(batch[0].revision, 3);
    let mut tx = store.pool.begin().await?;
    sqlx::query(
        "UPDATE rooms SET room_json=json_set(room_json,'$.status','closed') WHERE room_id='second'",
    )
    .execute(&mut *tx)
    .await?;
    room_changed(&mut tx, "second").await?;
    tx.commit().await?;
    let batch = store
        .take_member_projection_batch(now.timestamp() + 60, 20)
        .await?;
    assert_eq!(batch[0].state, "removed");
    Ok(())
}

#[tokio::test]
async fn batch_is_sixteen_and_jitter_stays_within_twenty_percent() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 30, now).await;
    for index in 0..17 {
        let member = crate::MemberAdmission {
            secure: None,
            projection_id: format!("projection-{index}"),
            issuer: "https://central.example".into(),
            person_id: format!("person-{index}"),
            display_name: "name".into(),
            registration_epoch: "epoch".into(),
            challenge_fingerprint: [4; 32],
            challenge_expires_at: now + Duration::minutes(5),
        };
        assert!(matches!(
            store
                .admit_human(
                    &prepared([2; 32], [3; 32], &uuid::Uuid::new_v4().to_string(), "name")
                        .with_member(member),
                    now
                )
                .await?,
            crate::HumanAdmissionDecision::Admitted(_)
        ));
    }
    let batch = store
        .take_member_projection_batch(now.timestamp(), 0)
        .await?;
    assert_eq!(batch.len(), 16);
    let deadline: i64 = sqlx::query_scalar("SELECT next_attempt_at FROM member_projection_sender")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(deadline, now.timestamp() + 48);
    store
        .finish_member_projection_batch(&batch, false, now.timestamp())
        .await?;
    let rest = store
        .take_member_projection_batch(now.timestamp() + 60, 40)
        .await?;
    assert_eq!(rest.len(), 1);
    let deadline: i64 = sqlx::query_scalar("SELECT next_attempt_at FROM member_projection_sender")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(deadline, now.timestamp() + 60 + 72);
    Ok(())
}

#[tokio::test]
async fn v83_upgrade_keeps_rooms_and_sender_deadline_survives_reopen() -> TestResult {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("runtime.sqlite3");
    let store = SqliteStore::open_path(&path).await?;
    store
        .bootstrap_local_authority("a7e0d675-3928-423e-837a-970805bfcfaa", "Name")
        .await?;
    store
        .create_room_for_local_operator("c076dc70-9d01-4080-841f-32199f446deb", "kept", "Kept")
        .await?;
    let before: String = sqlx::query_scalar("SELECT room_json FROM rooms WHERE room_id='kept'")
        .fetch_one(&store.pool)
        .await?;
    for sql in [
        "ALTER TABLE attendee_connections DROP COLUMN execution_os",
        "DROP TABLE member_projection_outbox",
        "DROP TABLE member_projection_sender",
        "UPDATE runtime_metadata SET value='83' WHERE key='schema_version'",
    ] {
        sqlx::query(sql).execute(&store.pool).await?;
    }
    store.close().await?;
    let store = SqliteStore::open_path(&path).await?;
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT room_json FROM rooms WHERE room_id='kept'")
            .fetch_one(&store.pool)
            .await?,
        before
    );
    sqlx::query("INSERT INTO member_projection_sender VALUES(1,123456,3,1,48)")
        .execute(&store.pool)
        .await?;
    store.close().await?;
    let store = SqliteStore::open_path(&path).await?;
    let record = sqlx::query("SELECT * FROM member_projection_sender")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(record.try_get::<i64, _>("next_attempt_at")?, 123_456);
    assert_eq!(record.try_get::<i64, _>("attempts")?, 48);
    Ok(())
}
