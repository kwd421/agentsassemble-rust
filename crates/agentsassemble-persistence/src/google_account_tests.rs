use crate::{
    AccountAuthority, AccountIdentity, HumanSessionAuthorization, PersistenceError, SqliteStore,
    human_admission_store::tests::{admitted, fixture, insert_invite, prepared},
};
use agentsassemble_domain::ParticipantStatus;
use chrono::Utc;
use serde_json::json;

fn checked<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("account fixture: {error:?}"))
}
async fn device(store: &SqliteStore, fingerprint: u8) -> AccountIdentity {
    checked(
        store
            .account_identity(AccountAuthority::BrowserDevice([fingerprint; 32]))
            .await,
    )
}

#[tokio::test]
async fn google_link_supports_multiple_devices_restart_and_link_only_disconnect() {
    let directory = checked(tempfile::tempdir());
    let path = directory.path().join("accounts.sqlite3");
    let store = checked(SqliteStore::open_path(&path).await);
    assert!(
        store
            .account_identity(AccountAuthority::BrowserDevice([1; 32]))
            .await
            .is_err()
    );
    checked(
        store
            .bootstrap_local_authority("55555555-5555-4555-8555-555555555555", "Host")
            .await,
    );
    let first = checked(
        store
            .connect_google_account(&device(&store, 1).await, &[11; 32], false)
            .await,
    );
    let second = checked(
        store
            .connect_google_account(&device(&store, 2).await, &[11; 32], false)
            .await,
    );
    assert_eq!(first.user.user_id, second.user.user_id);
    assert_eq!(first.account, second.account);
    let identity = device(&store, 1).await;
    assert!(matches!(
        store
            .connect_google_account(&identity, &[12; 32], false)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"account_link_conflict")
    ));
    checked(store.disconnect_google_account(&identity).await);
    assert!(checked(store.google_account(&device(&store, 2).await).await).is_none());
    assert_eq!(
        checked(device(&store, 2).await.user().ok_or("user")).profile,
        first.user.profile
    );
    drop(store);
    let store = checked(SqliteStore::open_path(&path).await);
    let restored = device(&store, 2).await;
    assert_eq!(
        checked(restored.user().ok_or("user")).user_id,
        first.user.user_id
    );
    let linked = checked(
        store
            .connect_google_account(&restored, &[11; 32], false)
            .await,
    );
    assert_eq!(linked.account, first.account);
}

#[tokio::test]
async fn confirmed_guest_switch_is_atomic_and_preserves_public_history() {
    let (store, guest, authorization) = admitted_guest().await;
    let target = checked(
        store
            .connect_google_account(&device(&store, 2).await, &[12; 32], false)
            .await,
    );
    checked(
        store
            .execute_authorized_message_with_turn(
                crate::RoomMutationAuthority::HumanSession(&authorization),
                "account-history",
                "message.send",
                &json!({"content": "Preserved guest history"}),
            )
            .await,
    );
    assert!(matches!(
        store.connect_google_account(&guest, &[12; 32], false).await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"account_switch_confirmation_required")
    ));
    checked(sqlx::query("CREATE TRIGGER reject_guest_retirement BEFORE DELETE ON user_profiles BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END").execute(&store.pool).await);
    assert!(
        store
            .connect_google_account(&guest, &[12; 32], true)
            .await
            .is_err()
    );
    checked(
        store
            .revalidate_human_session_authorization(&authorization)
            .await,
    );
    assert!(checked(store.google_account(&guest).await).is_none());
    checked(
        sqlx::query("DROP TRIGGER reject_guest_retirement")
            .execute(&store.pool)
            .await,
    );
    let switched = checked(store.connect_google_account(&guest, &[12; 32], true).await);
    assert!(switched.identity_switched);
    assert_eq!(switched.user.user_id, target.user.user_id);
    assert_eq!(
        switched.revoked_sessions,
        vec![(
            authorization.principal().room_id.clone(),
            *authorization.session_fingerprint()
        )]
    );
    assert_eq!(switched.events.len(), 1);
    assert_eq!(switched.events[0].event_type, "participant_left");
    assert!(
        store
            .revalidate_human_session_authorization(&authorization)
            .await
            .is_err()
    );
    assert!(store.google_account(&guest).await.is_err());
    assert_eq!(
        checked(device(&store, 1).await.user().ok_or("user")).user_id,
        target.user.user_id
    );
    let participant_json: String = checked(
        sqlx::query_scalar("SELECT participant_json FROM participants WHERE participant_id = ?")
            .bind(&authorization.principal().participant_id)
            .fetch_one(&store.pool)
            .await,
    );
    let participant: agentsassemble_domain::Participant =
        checked(serde_json::from_str(&participant_json));
    assert_eq!(participant.status, ParticipantStatus::Left);
    let messages: i64 = checked(
        sqlx::query_scalar("SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.participant_id') = ? AND json_extract(event_json, '$.content') = 'Preserved guest history'")
            .bind(&authorization.principal().participant_id)
            .fetch_one(&store.pool)
            .await,
    );
    assert_eq!(messages, 1);
}

#[tokio::test]
async fn account_proof_cannot_retire_operator_or_cross_a_session_device_boundary() {
    let (store, _, authorization) = admitted_guest().await;
    assert!(
        store
            .account_identity(AccountAuthority::HumanSession {
                authorization,
                browser_fingerprint: [99; 32]
            })
            .await
            .is_err()
    );
    checked(
        store
            .connect_google_account(&device(&store, 2).await, &[12; 32], false)
            .await,
    );
    let operator = checked(
        store
            .account_identity(AccountAuthority::LocalOperator)
            .await,
    );
    assert!(matches!(
        store
            .connect_google_account(&operator, &[12; 32], true)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"account_switch_operator_forbidden")
    ));
    checked(store.local_operator_profile().await);
    checked(
        store
            .connect_google_account(&operator, &[13; 32], false)
            .await,
    );
    assert!(matches!(
        store
            .connect_google_account(&device(&store, 3).await, &[13; 32], true)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"account_operator_boundary")
    ));
    assert!(device(&store, 3).await.user().is_none());
    assert!(checked(store.google_account(&operator).await).is_some());
}

async fn admitted_guest() -> (SqliteStore, AccountIdentity, HumanSessionAuthorization) {
    let (store, _) = fixture().await;
    let now = Utc::now();
    insert_invite(&store, [21; 32], [22; 32], "account-invite", 0, now).await;
    let input = prepared(
        [22; 32],
        [1; 32],
        "11111111-1111-4111-8111-111111111111",
        "Guest",
    );
    admitted(checked(store.admit_human(&input, now).await));
    let fingerprint: Vec<u8> = checked(
        sqlx::query_scalar("SELECT session_fingerprint FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await,
    );
    let fingerprint: [u8; 32] = checked(fingerprint.try_into());
    let authorization = checked(store.authorize_human_session(&fingerprint).await);
    let identity = checked(
        store
            .account_identity(AccountAuthority::HumanSession {
                authorization: authorization.clone(),
                browser_fingerprint: [1; 32],
            })
            .await,
    );
    (store, identity, authorization)
}
