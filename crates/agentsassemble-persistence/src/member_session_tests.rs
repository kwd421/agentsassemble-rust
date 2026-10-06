use super::{TestResult, admitted, fixture, insert_invite, member};
use crate::{HumanAdmissionDecision, HumanAdmissionRejection, SqliteStore};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

fn fingerprint(commit: &crate::HumanAdmissionCommit) -> [u8; 32] {
    Sha256::digest(commit.session_bearer().as_bytes()).into()
}

async fn counts(store: &SqliteStore) -> Result<(i64, i64), sqlx::Error> {
    sqlx::query_as("SELECT COUNT(*), SUM(state = 'active') FROM human_room_sessions WHERE member_admission_id IS NOT NULL")
        .fetch_one(&store.pool).await
}

#[tokio::test]
async fn same_device_replaces_only_itself_and_tombstone_retry_is_rejected() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let request = member(2, 3, "Member");
    let old = admitted(store.admit_human(&request, now).await?);
    let other = admitted(store.admit_human(&member(2, 4, "Member"), now).await?);
    let next = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    assert_eq!(next.replaced_session_fingerprints(), &[fingerprint(&old)]);
    assert!(
        store
            .authorize_human_session(&fingerprint(&old))
            .await
            .is_err()
    );
    store.authorize_human_session(&fingerprint(&other)).await?;
    store.authorize_human_session(&fingerprint(&next)).await?;
    assert!(matches!(
        store.admit_human(&request, now).await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
    ));
    assert_eq!(counts(&store).await?, (3, 2));
    Ok(())
}

#[tokio::test]
async fn lru_uses_successful_authority_activity_and_bounds_all_history() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let mut issued = Vec::new();
    for browser in 3..11 {
        issued.push(admitted(
            store
                .admit_human(
                    &member(2, browser, "Member"),
                    now + Duration::seconds(i64::from(browser)),
                )
                .await?,
        ));
    }
    // Touch the oldest device through the shared real authorization resolver.
    let mut tx = store.pool.begin().await?;
    assert!(matches!(
        crate::human_session_authority::resolve_human_session(
            &mut tx,
            &fingerprint(&issued[0]),
            None,
            now + Duration::seconds(20)
        )
        .await?,
        crate::human_session_authority::ResolvedHumanSession::Live { .. }
    ));
    tx.commit().await?;
    let ninth = admitted(
        store
            .admit_human(&member(2, 11, "Member"), now + Duration::seconds(21))
            .await?,
    );
    assert_eq!(
        ninth.replaced_session_fingerprints(),
        &[fingerprint(&issued[1])]
    );
    assert_eq!(counts(&store).await?, (9, 8));
    assert!(
        store
            .authorize_human_session(&fingerprint(&issued[1]))
            .await
            .is_err()
    );
    store
        .authorize_human_session(&fingerprint(&issued[0]))
        .await?;
    for browser in 12..90 {
        admitted(
            store
                .admit_human(
                    &member(2, browser, "Member"),
                    now + Duration::seconds(i64::from(browser)),
                )
                .await?,
        );
        let (rows, active) = counts(&store).await?;
        assert!(rows <= 40);
        assert_eq!(active, 8);
    }
    assert_eq!(counts(&store).await?, (40, 8));
    Ok(())
}

#[tokio::test]
async fn pruned_retry_cannot_regenerate_old_bearer_even_with_fresh_challenge() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    let mut old_request = member(2, 3, "Member");
    let old = admitted(
        store
            .admit_human(&old_request, now + Duration::seconds(1))
            .await?,
    );
    for second in 2..40 {
        admitted(
            store
                .admit_human(&member(2, 3, "Member"), now + Duration::seconds(second))
                .await?,
        );
    }
    assert_eq!(counts(&store).await?, (33, 1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM human_room_sessions WHERE session_fingerprint = ?"
        )
        .bind(fingerprint(&old).as_slice())
        .fetch_one(&store.pool)
        .await?,
        0
    );
    assert!(matches!(
        store
            .admit_human(&old_request, now + Duration::seconds(41))
            .await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::SessionUnavailable)
    ));
    let authority = old_request.member.as_mut().ok_or("member")?;
    authority.challenge_fingerprint = [99; 32];
    authority.challenge_expires_at = now + Duration::seconds(400);
    let fresh = admitted(
        store
            .admit_human(&old_request, now + Duration::seconds(101))
            .await?,
    );
    let different = old.session_bearer() != fresh.session_bearer();
    assert!(different);
    assert!(
        store
            .authorize_human_session(&fingerprint(&old))
            .await
            .is_err()
    );
    store.authorize_human_session(&fingerprint(&fresh)).await?;
    assert_eq!(counts(&store).await?, (33, 1));
    Ok(())
}

#[tokio::test]
async fn v82_upgrade_preserves_legacy_live_authority_and_bounds_ended_history() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 1, now).await;
    let original = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    sqlx::query("DROP TABLE member_projection_outbox")
        .execute(&store.pool)
        .await?;
    sqlx::query("DROP TABLE member_projection_sender")
        .execute(&store.pool)
        .await?;
    // Construct actual v82 columns/indexes with a legacy seed-backed live session.
    for sql in [
        "DROP INDEX human_room_sessions_member_device_idx",
        "DROP INDEX human_room_sessions_member_history_idx",
        "DROP INDEX human_room_sessions_active_participant_idx",
        "ALTER TABLE human_room_sessions DROP COLUMN member_challenge",
        "ALTER TABLE human_room_sessions DROP COLUMN member_challenge_expires_at",
        "ALTER TABLE human_room_sessions DROP COLUMN member_last_used_at",
        "ALTER TABLE member_admissions DROP COLUMN replay_floor",
        "CREATE UNIQUE INDEX human_room_sessions_active_participant_idx ON human_room_sessions(room_id, participant_id) WHERE state = 'active'",
        "UPDATE runtime_metadata SET value = '82' WHERE key = 'schema_version'",
    ] {
        sqlx::query(sql).execute(&store.pool).await?;
    }
    for index in 0..50 {
        sqlx::query("INSERT INTO human_room_sessions(admission_key, key_kind, member_admission_id, first_request_id, invite_id, payload_hash, session_fingerprint, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, result_json, admitted_at, expires_at, state) SELECT ?, key_kind, member_admission_id, first_request_id, invite_id, payload_hash, ?, room_id, user_id, participant_id, client_kind, invite_scope, browser_credential_fingerprint, result_json, admitted_at - ?, expires_at, 'ended' FROM human_room_sessions WHERE state = 'active'")
            .bind(Sha256::digest(format!("key-{index}")).as_slice()).bind(Sha256::digest(format!("fingerprint-{index}")).as_slice()).bind(index + 1).execute(&store.pool).await?;
    }
    crate::schema_version::validate_schema_version(&store.pool).await?;
    crate::schema_version::upgrade_schema(&store.pool).await?;
    assert_eq!(counts(&store).await?, (33, 1));
    store
        .authorize_human_session(&fingerprint(&original))
        .await?;
    admitted(
        store
            .admit_human(&member(2, 4, "Member"), Utc::now())
            .await?,
    );
    assert_eq!(counts(&store).await?, (34, 2));
    store
        .authorize_human_session(&fingerprint(&original))
        .await?;
    crate::schema_version::upgrade_schema(&store.pool).await?;
    Ok(())
}

#[tokio::test]
async fn other_devices_count_toward_room_capacity_and_rejection_is_atomic() -> TestResult {
    let (store, now) = fixture().await;
    store.set_registration_epoch(Some("epoch")).await?;
    insert_invite(&store, [1; 32], [2; 32], "guest", 200, now).await;
    let original = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    for index in 0..111 {
        let mut request = member(2, 3, "Other member");
        request.member.as_mut().ok_or("member")?.person_id = format!("other-{index}");
        admitted(store.admit_human(&request, now).await?);
    }
    assert!(matches!(
        store.admit_human(&member(2, 4, "Member"), now).await?,
        HumanAdmissionDecision::Rejected(HumanAdmissionRejection::CapacityReached)
    ));
    store
        .authorize_human_session(&fingerprint(&original))
        .await?;
    assert_eq!(counts(&store).await?, (112, 112));
    let next = admitted(store.admit_human(&member(2, 3, "Member"), now).await?);
    assert_eq!(
        next.replaced_session_fingerprints(),
        &[fingerprint(&original)]
    );
    assert_eq!(counts(&store).await?, (113, 112));
    Ok(())
}
