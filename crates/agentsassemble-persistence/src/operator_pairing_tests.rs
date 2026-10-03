use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID};
use chrono::{Duration, Utc};

use super::{CentralOwnerSessionRequest, PAIRING_TTL, SESSION_TTL, revalidate_operator_session};
use crate::{LocalRoomManagerAuthority, PersistenceError, SqliteStore};

const LOCAL: crate::ServerOwnerAuthority = crate::ServerOwnerAuthority::LocalOperator;
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

fn code<T>(result: Result<T, PersistenceError>) -> std::borrow::Cow<'static, str> {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => code,
        Err(error) => panic!("unexpected failure: {error}"),
        Ok(_) => panic!("unexpected success"),
    }
}

#[tokio::test]
async fn central_owner_session_is_room_device_origin_and_grant_expiry_bound() {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    let expires_at = now + Duration::minutes(4);
    let request = CentralOwnerSessionRequest::new(
        "general",
        manager.room_uid,
        &[7; 32],
        &[8; 32],
        ORIGIN,
        expires_at,
        now,
    );
    let first = store
        .create_central_owner_session(&LOCAL, &request)
        .await
        .unwrap_or_else(|error| panic!("central session: {error}"));
    let replay = store
        .create_central_owner_session(&LOCAL, &request)
        .await
        .unwrap_or_else(|error| panic!("central replay: {error}"));
    assert_eq!(first.session_bearer, replay.session_bearer);
    assert_eq!(first.authorization.expires_at(), expires_at);
    verify_owner_profile_authority(&store, &manager, &first.authorization, now).await;

    assert_eq!(
        code(
            store
                .create_central_owner_session(
                    &LOCAL,
                    &CentralOwnerSessionRequest::new(
                        "general",
                        manager.room_uid,
                        &[7; 32],
                        &[9; 32],
                        ORIGIN,
                        expires_at,
                        now,
                    )
                )
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .create_central_owner_session(
                    &LOCAL,
                    &CentralOwnerSessionRequest::new(
                        "general",
                        uuid::Uuid::new_v4(),
                        &[6; 32],
                        &[8; 32],
                        ORIGIN,
                        expires_at,
                        now,
                    )
                )
                .await
        ),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .create_central_owner_session(
                    &LOCAL,
                    &CentralOwnerSessionRequest::new(
                        "general",
                        manager.room_uid,
                        &[10; 32],
                        &[8; 32],
                        ORIGIN,
                        now + Duration::minutes(6),
                        now,
                    )
                )
                .await
        ),
        "session_revoked"
    );
}

async fn verify_owner_profile_authority(
    store: &SqliteStore,
    manager: &LocalRoomManagerAuthority,
    authorization: &super::OperatorSessionAuthorization,
    now: chrono::DateTime<Utc>,
) {
    assert!(authorization.is_central_owner());
    let owner = crate::ServerOwnerAuthority::CentralOwner(Box::new(authorization.clone()));
    let profile = store
        .server_owner_profile(&owner)
        .await
        .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
    let patch = agentsassemble_domain::UserProfilePatch {
        display_name: Some("Owner from web".to_owned()),
        ..Default::default()
    };
    let updated = store
        .update_server_owner_profile(&owner, profile.revision, patch.clone())
        .await
        .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
    assert_eq!(
        updated.profile,
        store
            .local_operator_profile()
            .await
            .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"))
    );
    store
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local((manager).clone()),
            &[20; 32],
            ORIGIN,
            now,
        )
        .await
        .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
    let ordinary = store
        .redeem_operator_pairing(&[20; 32], &[21; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
    assert!(!ordinary.authorization.is_central_owner());
    let ordinary_owner =
        crate::ServerOwnerAuthority::CentralOwner(Box::new(ordinary.authorization));
    assert_eq!(
        code(store.saved_friends(&ordinary_owner).await),
        "session_revoked"
    );

    assert_eq!(
        code(store.server_owner_profile(&ordinary_owner).await),
        "session_revoked"
    );
    assert_eq!(
        code(
            store
                .update_server_owner_profile(
                    &ordinary_owner,
                    updated.profile.revision,
                    patch.clone()
                )
                .await
        ),
        "session_revoked"
    );
    for changed in [
        "UPDATE operator_pairings SET revoked = 1 WHERE central_owner = 1",
        "UPDATE operator_pairings SET session_expires_at = 946684800 WHERE central_owner = 1",
        "UPDATE operator_pairings SET target_origin = 'https://other.example.test' WHERE central_owner = 1",
        "UPDATE operator_pairings SET device_fingerprint = zeroblob(32) WHERE central_owner = 1",
        "UPDATE operator_pairings SET central_owner = 0 WHERE central_owner = 1",
    ] {
        let mut tx = store
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
        sqlx::query(changed)
            .execute(&mut *tx)
            .await
            .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
        assert_eq!(
            code(super::revalidate_central_owner_session(&mut tx, authorization).await),
            "session_revoked"
        );
        assert_eq!(code(owner.revalidate(&mut tx).await), "session_revoked");
        tx.rollback()
            .await
            .unwrap_or_else(|error| panic!("owner profile verification: {error:?}"));
    }
}

#[tokio::test]
async fn archive_revokes_used_and_unused_pairings_permanently_after_restore() {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now();
    for token in [[1; 32], [3; 32]] {
        store
            .create_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager.clone()),
                &token,
                ORIGIN,
                now,
            )
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
            crate::RoomMutationAuthority::OperatorSession(&paired.authorization),
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
    assert_eq!(
        code(
            store
                .execute_room_lifecycle(
                    crate::RoomMutationAuthority::OperatorSession(&paired.authorization),
                    "archive-paired-room",
                    "room.archive",
                    &serde_json::json!({"room_uid": room.room_uid, "archived": true}),
                )
                .await
        ),
        "session_revoked"
    );
    store
        .execute_room_lifecycle(
            TrustedPrincipal(principal),
            "restore-paired-room",
            "room.archive",
            &serde_json::json!({"room_uid": room.room_uid, "archived": false}),
        )
        .await
        .unwrap_or_else(|error| panic!("restore: {error}"));
    assert_eq!(
        code(store.execute_room_delete(
            crate::RoomMutationAuthority::OperatorSession(&paired.authorization),
            "delete-after-revoke",
            &serde_json::json!({"room_uid": room.room_uid, "confirmation_name": room.label}),
        ).await),
        "session_revoked"
    );
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
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager.clone()),
            &[1; 32],
            ORIGIN,
            now,
        )
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
            .revoke_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager.clone()),
                pairing.pairing_id
            )
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
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager.clone()),
            &[1; 32],
            ORIGIN,
            now,
        )
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
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager.clone()),
            &[1; 32],
            ORIGIN,
            now,
        )
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
        .create_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager.clone()),
            &[1; 32],
            ORIGIN,
            now,
        )
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
        .revoke_operator_pairing(
            &crate::RoomManagerAuthority::Local(manager.clone()),
            pairing.pairing_id,
        )
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

#[tokio::test]
async fn paired_attendee_observes_parent_expiry_membership_and_exact_host_room()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{AttendeeAdmissionRequest, CompanionInviteRequest, RoomSessionAuthorization};
    use sha2::{Digest, Sha256};
    use uuid::Uuid;

    for invalidation in ["expiry", "muted", "room_uid", "authority_lineage_id"] {
        let (store, manager) = fixture("sqlite::memory:").await;
        let now = Utc::now();
        // A nearly expired real pairing makes the parent's lifetime, rather than
        // the attendee's ordinary hour, the limiting authority.
        let paired_at = now - SESSION_TTL + Duration::seconds(20);
        store
            .create_operator_pairing(
                &crate::RoomManagerAuthority::Local(manager.clone()),
                &[1; 32],
                ORIGIN,
                paired_at,
            )
            .await?;
        let paired = store
            .redeem_operator_pairing(&[1; 32], &[2; 32], ORIGIN, paired_at)
            .await?;
        let issuer = RoomSessionAuthorization::Operator(paired.authorization);
        let request = || CompanionInviteRequest {
            request_id: Uuid::new_v4(),
            provider_kind: "codex_live_session",
            display_name: "Paired AI",
        };
        let invite = store
            .create_companion_attendee_invite(&issuer, request(), now)
            .await?;
        assert_eq!(invite.expires_at, issuer.expires_at());
        let fingerprint: [u8; 32] = Sha256::digest(invite.invite_bearer.as_bytes()).into();
        let admission = store
            .admit_attendee(
                AttendeeAdmissionRequest {
                    invite_fingerprint: &fingerprint,
                    client_fingerprint: &[3; 32],
                    request_id: Uuid::new_v4(),
                    provider_kind: "codex_live_session",
                    display_name: "Paired AI",
                },
                now,
            )
            .await?;
        assert_eq!(admission.authorization.expires_at(), issuer.expires_at());
        store
            .revalidate_attendee_session(&admission.authorization, now)
            .await?;
        let checked_at = match invalidation {
            "expiry" => issuer.expires_at(),
            "muted" => {
                sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.muted',json('true')) WHERE participant_id=?")
                    .bind(LOCAL_OPERATOR_PARTICIPANT_ID).execute(&store.pool).await?;
                now
            }
            "room_uid" => {
                sqlx::query("UPDATE operator_pairings SET room_uid=?")
                    .bind(Uuid::new_v4().to_string())
                    .execute(&store.pool)
                    .await?;
                now
            }
            "authority_lineage_id" => {
                sqlx::query("UPDATE operator_pairings SET authority_lineage_id=?")
                    .bind(Uuid::new_v4().to_string())
                    .execute(&store.pool)
                    .await?;
                now
            }
            _ => unreachable!(),
        };
        assert!(
            store
                .revalidate_attendee_session(&admission.authorization, checked_at)
                .await
                .is_err(),
            "{invalidation}"
        );
        assert!(
            store
                .create_companion_attendee_invite(&issuer, request(), checked_at)
                .await
                .is_err(),
            "{invalidation}"
        );
    }
    Ok(())
}
