use chrono::{Duration, Utc};

use crate::{
    CentralOwnerSessionRequest, OwnerConnectionBinding, OwnerConnectionLease,
    RoomSessionAuthorization, ServerOwnerAuthority, SqliteStore,
};

async fn setup() -> Result<(SqliteStore, OwnerConnectionBinding), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    let server = store.local_bootstrap_status().await?.server_id;
    let generation = store.next_central_endpoint_generation().await?;
    Ok((
        store,
        OwnerConnectionBinding {
            connection_id: format!("soc_{}", "a".repeat(43)),
            server_id: server,
            person_id: "central-person".into(),
            device_id: "central-device".into(),
            browser_fingerprint: [42; 32],
            origin: "https://owner.example.test".into(),
            generation,
            session_expires_at: Utc::now().timestamp() + 3600,
        },
    ))
}

fn lease(binding: OwnerConnectionBinding) -> Result<OwnerConnectionLease, crate::PersistenceError> {
    let now = Utc::now().timestamp();
    OwnerConnectionLease::verified(binding, now + 60, now + 20)
}

#[tokio::test]
async fn owner_parent_is_room_independent_and_renewal_preserves_room_credential()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let first = store.create_owner_session(&lease(binding.clone())?).await?;
    let replay = store.create_owner_session(&lease(binding.clone())?).await?;
    assert_eq!(first.session_bearer, replay.session_bearer);
    let owner_session = first.authorization;
    assert!(
        store
            .authorize_owner_session(owner_session.fingerprint(), &[41; 32], &binding.origin)
            .await
            .is_err()
    );
    assert!(
        store
            .authorize_owner_session(
                owner_session.fingerprint(),
                &[42; 32],
                "https://other.example.test"
            )
            .await
            .is_err()
    );
    assert!(
        store
            .list_room_directory_for_owner(
                &ServerOwnerAuthority::CentralSession(owner_session.clone()),
                true
            )
            .await?
            .1
            .is_empty()
    );
    let created = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let request = CentralOwnerSessionRequest::new(
        "room",
        created.room.room_uid,
        &[17; 32],
        &[42; 32],
        &binding.origin,
        chrono::DateTime::from_timestamp(binding.session_expires_at, 0).ok_or("timestamp")?,
        Utc::now(),
    );
    let room = store
        .create_central_owner_session(
            &ServerOwnerAuthority::CentralSession(owner_session.clone()),
            &request,
        )
        .await?;
    let expected = RoomSessionAuthorization::Operator(room.authorization.clone());
    assert!(room.authorization.expires_at() > Utc::now() + Duration::minutes(5));
    store
        .renew_owner_session(&owner_session, &lease(binding.clone())?)
        .await?;
    let current = store
        .revalidate_room_session_authorization(&expected)
        .await?;
    assert_eq!(
        current.session_fingerprint(),
        expected.session_fingerprint()
    );
    assert_eq!(current.expires_at(), expected.expires_at());
    store
        .revoke_owner_session(owner_session.fingerprint())
        .await?;
    assert!(
        store
            .revalidate_room_session_authorization(&expected)
            .await
            .is_err()
    );
    assert!(
        store
            .validate_server_owner(&ServerOwnerAuthority::CentralSession(owner_session.clone()))
            .await
            .is_err()
    );
    assert!(
        store
            .renew_owner_session(&owner_session, &lease(binding)?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn expired_parent_and_endpoint_replacement_end_authority_without_promoting_pairings()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let issued = store.create_owner_session(&lease(binding.clone())?).await?;
    let owner_session = issued.authorization;
    sqlx::query("UPDATE central_owner_sessions SET expires_at = ?")
        .bind(Utc::now().timestamp())
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .authorize_owner_session(owner_session.fingerprint(), &[42; 32], &binding.origin)
            .await
            .is_err()
    );
    assert!(
        store
            .renew_owner_session(&owner_session, &lease(binding.clone())?)
            .await
            .is_err()
    );
    assert!(
        store
            .create_owner_session(&lease(binding.clone())?)
            .await
            .is_err()
    );
    let (store, binding) = setup().await?;
    let owner_session = store
        .create_owner_session(&lease(binding.clone())?)
        .await?
        .authorization;
    store.next_central_endpoint_generation().await?;
    assert!(
        store
            .authorize_owner_session(owner_session.fingerprint(), &[42; 32], &binding.origin)
            .await
            .is_err()
    );
    let now = Utc::now().timestamp();
    assert!(OwnerConnectionLease::verified(binding, now + 61, now + 20).is_err());
    Ok(())
}
