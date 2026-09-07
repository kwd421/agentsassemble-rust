use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
use chrono::{Duration, Utc};

use super::{PAIRING_TTL, SESSION_TTL, revalidate_operator_session};
use crate::{LocalRoomManagerAuthority, PersistenceError, SqliteStore};

const ORIGIN: &str = "https://room.example.test";

async fn fixture(url: &str) -> (SqliteStore, LocalRoomManagerAuthority) {
    let store = SqliteStore::open(url)
        .await
        .unwrap_or_else(|error| panic!("store: {error}"));
    store
        .bootstrap_local_authority("baef1a5c-c6f6-4d7b-8f5c-e500ef84a813", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error}"));
    store
        .create_room_for_local_operator(
            "897948ca-a367-4741-8fe4-9194086a0a51",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("room: {error}"));
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("manager: {error}"));
    (store, manager)
}

fn code<T>(result: Result<T, PersistenceError>) -> &'static str {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => code,
        Err(error) => panic!("unexpected failure: {error}"),
        Ok(_) => panic!("unexpected success"),
    }
}

#[tokio::test]
async fn archive_revokes_used_and_unused_pairings_permanently_after_restore() {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    for token in [[1; 32], [3; 32]] {
        store
            .create_operator_pairing(&manager, &token, ORIGIN, now)
            .await
            .unwrap_or_else(|error| panic!("grant: {error}"));
    }
    let paired = store
        .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("redeem: {error}"));
    let principal = paired.authorization.principal();
    let room = store
        .snapshot("general", 0, 20)
        .await
        .unwrap_or_else(|error| panic!("room: {error}"))
        .room;
    let archived = store
        .execute_room_lifecycle(
            principal,
            "archive-paired-room",
            "room.archive",
            &serde_json::json!({"room_uid": room.room_uid, "archived": true}),
        )
        .await
        .unwrap_or_else(|error| panic!("archive: {error}"));
    assert_eq!(
        archived.revoked_session_fingerprints,
        vec![*paired.authorization.session_fingerprint()]
    );
    store
        .execute_room_lifecycle(
            principal,
            "restore-paired-room",
            "room.archive",
            &serde_json::json!({"room_uid": room.room_uid, "archived": false}),
        )
        .await
        .unwrap_or_else(|error| panic!("restore: {error}"));
    for token in [[1; 32], [3; 32]] {
        assert_eq!(
            code(
                store
                    .redeem_operator_pairing(&token, &[2; 32], ORIGIN, now)
                    .await
            ),
            "session_revoked"
        );
    }
    assert_eq!(
        code(
            store
                .authorize_operator_session(
                    paired.authorization.session_fingerprint(),
                    &[2; 32],
                    ORIGIN
                )
                .await
        ),
        "session_revoked"
    );
}

#[tokio::test]
async fn concurrent_redemption_has_one_device_owner_and_retry_survives_token_expiry() {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    let pairing = store
        .create_operator_pairing(&manager, &[1; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("grant: {error}"));
    let (first, second) = tokio::join!(
        store.redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now),
        store.redeem_operator_pairing(&[1; 32], &[3; 32], ORIGIN, now),
    );
    let (winner, device, loser) = match (first, second) {
        (Ok(value), other) => (value, [2; 32], other),
        (other, Ok(value)) => (value, [3; 32], other),
        _ => panic!("neither device admitted"),
    };
    assert_eq!(code(loser), "pairing_already_used");
    let retry = store
        .redeem_operator_pairing(
            &[1; 32],
            &device,
            ORIGIN,
            now + PAIRING_TTL + Duration::seconds(1),
        )
        .await
        .unwrap_or_else(|error| panic!("same device retry: {error}"));
    assert_eq!(retry.session_bearer, winner.session_bearer);
    assert!(retry.authorization.principal().is_operator);
    assert_eq!(retry.authorization.principal().room_id, "general");
    let fingerprint = *retry.authorization.session_fingerprint();
    store
        .authorize_operator_session(&fingerprint, &device, ORIGIN)
        .await
        .unwrap_or_else(|error| panic!("live session: {error}"));
    assert_eq!(
        code(
            store
                .authorize_operator_session(&fingerprint, &[4; 32], ORIGIN)
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .authorize_operator_session(&fingerprint, &device, "https://foreign.test")
                .await
        ),
        "session_revoked"
    );
    let mut tx = store
        .pool
        .begin()
        .await
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    revalidate_operator_session(&mut tx, &retry.authorization, now)
        .await
        .unwrap_or_else(|error| panic!("queued authorization: {error}"));
    tx.commit()
        .await
        .unwrap_or_else(|error| panic!("commit: {error}"));
    assert_eq!(
        store
            .revoke_operator_pairing(&manager, pairing.pairing_id)
            .await
            .unwrap_or_else(|error| panic!("revoke: {error}")),
        Some(fingerprint)
    );
    assert_eq!(
        code(
            store
                .redeem_operator_pairing(&[1; 32], &device, ORIGIN, now)
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .authorize_operator_session(&fingerprint, &device, ORIGIN)
                .await
        ),
        "session_revoked"
    );
}

#[tokio::test]
async fn expiry_origin_and_room_incarnation_fail_without_consuming_grant() {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    store
        .create_operator_pairing(&manager, &[1; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("grant: {error}"));
    assert_eq!(
        code(
            store
                .redeem_operator_pairing(&[1; 32], &[2; 32], "https://foreign.test", now)
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now + PAIRING_TTL)
                .await
        ),
        "session_revoked"
    );
    let unused: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM operator_pairings WHERE device_fingerprint IS NULL",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("unconsumed: {error}"));
    assert_eq!(unused, 1);
    let admitted = store
        .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("admitted: {error}"));
    assert_eq!(
        code(
            store
                .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now + SESSION_TTL)
                .await
        ),
        "session_revoked"
    );
    sqlx::query("UPDATE operator_pairings SET room_uid = ?")
        .bind(uuid::Uuid::new_v4().to_string())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("different room incarnation: {error}"));
    assert_eq!(
        code(
            store
                .authorize_operator_session(
                    admitted.authorization.session_fingerprint(),
                    &[2; 32],
                    ORIGIN
                )
                .await
        ),
        "session_revoked"
    );
}

#[tokio::test]
async fn ordinary_human_bearer_domain_cannot_match_operator_session() {
    use crate::session_bearer::{SessionBearerPurpose, derive_session_bearer};
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    store
        .create_operator_pairing(&manager, &[1; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("grant: {error}"));
    let paired = store
        .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("paired: {error}"));
    let human = derive_session_bearer(
        store.host_key.session_hmac_key(),
        &[1; 32],
        SessionBearerPurpose::HumanAdmission,
    );
    assert_ne!(human.bearer, paired.session_bearer);
    assert_eq!(
        code(
            store
                .authorize_operator_session(&human.fingerprint, &[2; 32], ORIGIN)
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .authorize_human_session(paired.authorization.session_fingerprint())
                .await
        ),
        "session_revoked"
    );
}

#[tokio::test]
async fn consumed_pairing_survives_restart_and_queued_authority_observes_revocation() {
    let directory = tempfile::tempdir().unwrap_or_else(|error| panic!("directory: {error}"));
    let url = format!("sqlite://{}", directory.path().join("pairing.db").display());
    let (store, manager) = fixture(&url).await;
    let now = Utc::now();
    let pairing = store
        .create_operator_pairing(&manager, &[1; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("grant: {error}"));
    let first = store
        .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("redeem: {error}"));
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open(&url)
        .await
        .unwrap_or_else(|error| panic!("reopen: {error}"));
    let retry = store
        .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("retry: {error}"));
    assert_eq!(retry.session_bearer, first.session_bearer);
    store
        .revoke_operator_pairing(&manager, pairing.pairing_id)
        .await
        .unwrap_or_else(|error| panic!("revoke: {error}"));
    let mut tx = store
        .pool
        .begin()
        .await
        .unwrap_or_else(|error| panic!("transaction: {error}"));
    assert_eq!(
        code(
            crate::RoomMutationAuthority::OperatorSession(&first.authorization)
                .resolve(&mut tx)
                .await
        ),
        "session_revoked"
    );
}
