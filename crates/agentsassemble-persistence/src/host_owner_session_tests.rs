use chrono::Utc;

use crate::{OwnerAdmission, OwnerAdmissionBinding, OwnerDeviceDescription, SqliteStore};

async fn setup() -> Result<(SqliteStore, OwnerAdmissionBinding), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    let server_id = store.local_bootstrap_status().await?.server_id;
    let generation = store.next_central_endpoint_generation().await?;
    Ok((
        store,
        OwnerAdmissionBinding {
            entry_fingerprint: [1; 32],
            server_id,
            person_id: "central-person".into(),
            device_id: "central-device".into(),
            browser_fingerprint: [42; 32],
            origin: "https://owner.example.test".into(),
            generation,
        },
    ))
}

fn admission(binding: OwnerAdmissionBinding) -> Result<OwnerAdmission, crate::PersistenceError> {
    OwnerAdmission::verified(binding, Utc::now().timestamp() + 300)
}

fn description() -> Result<OwnerDeviceDescription, crate::PersistenceError> {
    OwnerDeviceDescription::verified("Chrome · macOS".into(), "Chrome".into(), "macOS".into())
}

#[tokio::test]
async fn host_admission_has_no_clock_expiry_and_disconnected_entry_cannot_replay()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let entry = admission(binding.clone())?;
    let first = store.create_owner_session(&entry, &description()?).await?;
    let replay = store.create_owner_session(&entry, &description()?).await?;
    assert_eq!(first.session_bearer, replay.session_bearer);
    assert_eq!(
        first.authorization.session_id(),
        replay.authorization.session_id()
    );
    assert!(
        store
            .authorize_owner_session(
                first.authorization.fingerprint(),
                &[41; 32],
                &binding.origin
            )
            .await
            .is_err()
    );
    sqlx::query("UPDATE host_owner_sessions SET admission_expires_at = ?")
        .bind(Utc::now().timestamp() - 1)
        .execute(&store.pool)
        .await?;
    store
        .authorize_owner_session(
            first.authorization.fingerprint(),
            &[42; 32],
            &binding.origin,
        )
        .await?;
    store
        .disconnect_owner_session(first.authorization.fingerprint())
        .await?;
    assert!(
        store
            .authorize_owner_session(
                first.authorization.fingerprint(),
                &[42; 32],
                &binding.origin
            )
            .await
            .is_err()
    );
    assert!(
        store
            .create_owner_session(&entry, &description()?)
            .await
            .is_err()
    );
    assert!(
        OwnerDeviceDescription::verified("bad\nname".into(), "Chrome".into(), "macOS".into())
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn direct_owner_room_has_no_deadline_and_ends_with_its_root()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::{CentralOwnerSessionRequest, RoomSessionAuthorization, ServerOwnerAuthority};
    let (store, binding) = setup().await?;
    let admitted = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let owner = ServerOwnerAuthority::CentralSession(admitted.authorization.clone());
    let room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let issued = store
        .create_central_owner_session(
            &owner,
            &CentralOwnerSessionRequest::host_owned(
                "room",
                room.room.room_uid,
                &[17; 32],
                &[42; 32],
                &binding.origin,
                Utc::now(),
            ),
        )
        .await?;
    assert_eq!(issued.authorization.expires_at(), None);
    let expected = RoomSessionAuthorization::Operator(issued.authorization);
    store
        .revalidate_room_session_authorization(&expected)
        .await?;
    store
        .disconnect_owner_session(admitted.authorization.fingerprint())
        .await?;
    assert!(
        store
            .revalidate_room_session_authorization(&expected)
            .await
            .is_err()
    );
    Ok(())
}
