use agentsassemble_domain::InviteScope;
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};

use crate::{
    AccountAuthority, AccountIdentity, GuestRecoveryRequest, HumanSessionAuthorization,
    PersistenceError, SqliteStore,
    human_admission_store::tests::{admitted, fixture, insert_invite, prepared},
};

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("recovery fixture: {error:?}"))
}

fn fingerprint(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}

async fn admit(
    store: &SqliteStore,
    now: DateTime<Utc>,
) -> (HumanSessionAuthorization, AccountIdentity) {
    insert_invite(store, [1; 32], [2; 32], "recoverable-human", 1, now).await;
    let commit = admitted(checked(
        store
            .admit_human(
                &prepared(
                    [2; 32],
                    [3; 32],
                    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
                    "Recoverable",
                ),
                now,
            )
            .await,
    ));
    let session = checked(
        store
            .authorize_human_session(&fingerprint(commit.session_bearer()))
            .await,
    );
    let identity = checked(
        store
            .account_identity(AccountAuthority::HumanSession {
                authorization: session.clone(),
                browser_fingerprint: [3; 32],
            })
            .await,
    );
    (session, identity)
}

fn request<'a>(code: &'a [u8; 32], device: &'a [u8; 32]) -> GuestRecoveryRequest<'a> {
    GuestRecoveryRequest {
        fingerprint: code,
        device,
        room_id: "general",
        client_id: "browser-client",
    }
}

#[tokio::test]
async fn rotation_preserves_identity_without_invitation_authority() {
    let (store, now) = fixture().await;
    let (old_session, identity) = admit(&store, now).await;
    let retired = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    let code = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    assert!(
        store
            .redeem_guest_recovery_code(&request(&retired, &[4; 32]), now)
            .await
            .is_err()
    );
    // Recovery is existing membership authority, independent of an invitation's availability.
    checked(
        sqlx::query("UPDATE room_invites SET revoked = 1")
            .execute(&store.pool)
            .await,
    );
    let recovered = checked(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), now)
            .await,
    );
    assert_eq!(
        recovered.result.agent_id,
        old_session.principal().participant_id
    );
    assert_eq!(
        recovered.replaced_session_fingerprints,
        vec![*old_session.session_fingerprint()]
    );
    assert!(
        store
            .revalidate_human_session_authorization(&old_session)
            .await
            .is_err()
    );
    let current = checked(
        store
            .authorize_human_session(&fingerprint(&recovered.session_bearer))
            .await,
    );
    assert_eq!(
        current.principal().principal_id,
        old_session.principal().principal_id
    );
    assert_eq!(current.principal().invite_scope, InviteScope::ReadWrite);
    assert!(!current.principal().is_operator);
    let source: (String, Option<String>, Option<String>, Option<Vec<u8>>) = checked(sqlx::query_as(
        "SELECT key_kind, invite_id, first_request_id, payload_hash FROM human_room_sessions WHERE state = 'active'",
    ).fetch_one(&store.pool).await);
    assert_eq!(source, ("recovery".into(), None, None, None));
    assert_eq!(
        checked(
            sqlx::query_scalar::<_, i64>("SELECT use_count FROM room_invites")
                .fetch_one(&store.pool)
                .await
        ),
        1
    );
    let rows: (i64, i64) = checked(
        sqlx::query_as(
            "SELECT length(fingerprint), length(previous_fingerprint) FROM guest_recovery_codes",
        )
        .fetch_one(&store.pool)
        .await,
    );
    assert_eq!(rows, (32, 32));
}

#[tokio::test]
async fn same_device_receipt_survives_restart_and_retires_on_next_recovery() {
    let directory = checked(tempfile::tempdir());
    let path = directory.path().join("recovery.sqlite3");
    let store = checked(SqliteStore::open_path(&path).await);
    checked(
        store
            .bootstrap_local_authority("e5f63872-a170-4e34-98af-55940ff4a91a", "Host")
            .await,
    );
    checked(
        store
            .create_room_for_local_operator(
                "15ebaf41-12b9-4b30-94d1-d62435b30fba",
                "general",
                "General",
            )
            .await,
    );
    let now = Utc::now();
    let (_, identity) = admit(&store, now).await;
    let code = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    let recovered = checked(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), now)
            .await,
    );
    drop(store);
    let store = checked(SqliteStore::open_path(&path).await);
    let retry = checked(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), now)
            .await,
    );
    assert!(
        retry.session_bearer == recovered.session_bearer
            && retry.recovery_code == recovered.recovery_code
    );
    assert!(retry.replaced_session_fingerprints.is_empty());
    assert!(
        store
            .redeem_guest_recovery_code(&request(&code, &[5; 32]), now)
            .await
            .is_err()
    );
    let changed = GuestRecoveryRequest {
        client_id: "different-client",
        ..request(&code, &[4; 32])
    };
    assert!(
        store
            .redeem_guest_recovery_code(&changed, now)
            .await
            .is_err()
    );
    let next_code = fingerprint(&recovered.recovery_code);
    checked(
        store
            .redeem_guest_recovery_code(&request(&next_code, &[5; 32]), now + Duration::seconds(1))
            .await,
    );
    assert!(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), now)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn conflicts_and_transaction_failure_leave_code_and_prior_session_usable() {
    let (store, now) = fixture().await;
    let (session, identity) = admit(&store, now).await;
    let code = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    checked(
        sqlx::query("INSERT INTO human_device_credentials VALUES (?, ?, ?)")
            .bind([4_u8; 32].as_slice())
            .bind(agentsassemble_domain::LOCAL_OPERATOR_USER_ID)
            .bind(now.timestamp_micros())
            .execute(&store.pool)
            .await,
    );
    assert!(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), now)
            .await
            .is_err()
    );
    checked(sqlx::query("CREATE TRIGGER reject_recovery_session BEFORE INSERT ON human_room_sessions WHEN NEW.key_kind = 'recovery' BEGIN SELECT RAISE(ABORT, 'injected recovery failure'); END")
        .execute(&store.pool).await);
    assert!(
        store
            .redeem_guest_recovery_code(&request(&code, &[5; 32]), now)
            .await
            .is_err()
    );
    checked(store.revalidate_human_session_authorization(&session).await);
    assert_eq!(
        checked(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM human_device_credentials WHERE credential_fingerprint = ?"
            )
            .bind([5_u8; 32].as_slice())
            .fetch_one(&store.pool)
            .await
        ),
        0
    );
    checked(
        sqlx::query("DROP TRIGGER reject_recovery_session")
            .execute(&store.pool)
            .await,
    );
    checked(
        store
            .redeem_guest_recovery_code(&request(&code, &[5; 32]), now)
            .await,
    );
}

#[tokio::test]
async fn expired_sessions_can_recover_active_membership_but_cannot_revive_removed_members() {
    let (store, now) = fixture().await;
    let (session, identity) = admit(&store, now).await;
    let code = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    let future = now + Duration::hours(2);
    let recovered = checked(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), future)
            .await,
    );
    assert_eq!(
        recovered.result.agent_id,
        session.principal().participant_id
    );
    let next = fingerprint(&recovered.recovery_code);
    checked(sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.status', ?) WHERE participant_id = ?")
        .bind("kicked")
        .bind(&recovered.result.agent_id).execute(&store.pool).await);
    assert!(matches!(
        store
            .redeem_guest_recovery_code(&request(&next, &[5; 32]), future)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "recovery_membership_inactive",
            ..
        })
    ));
    assert!(
        store
            .redeem_guest_recovery_code(&request(&code, &[4; 32]), future)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn concurrent_devices_have_one_winner_and_native_authority_cannot_issue() {
    let (store, now) = fixture().await;
    let (_, identity) = admit(&store, now).await;
    let code = fingerprint(&checked(store.issue_guest_recovery_code(&identity).await));
    let left = request(&code, &[4; 32]);
    let right = request(&code, &[5; 32]);
    let (first, second) = tokio::join!(
        store.redeem_guest_recovery_code(&left, now),
        store.redeem_guest_recovery_code(&right, now),
    );
    assert_ne!(first.is_ok(), second.is_ok());
    let operator = checked(
        store
            .account_identity(AccountAuthority::LocalOperator)
            .await,
    );
    assert!(store.issue_guest_recovery_code(&operator).await.is_err());
}
