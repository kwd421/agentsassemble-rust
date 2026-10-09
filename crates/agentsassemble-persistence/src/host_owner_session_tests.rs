use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};

use crate::{
    CentralOwnerSessionRequest, OwnerAdmission, OwnerAdmissionBinding, OwnerDeviceDescription,
    RoomManagerAuthority, RoomSessionAuthorization, ServerOwnerAuthority, SqliteStore,
};

pub(crate) async fn setup()
-> Result<(SqliteStore, OwnerAdmissionBinding), Box<dyn std::error::Error>> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&uuid::Uuid::new_v4().to_string(), "Owner")
        .await?;
    let server_id = store.local_bootstrap_status().await?.server_id;
    let generation = store.next_central_endpoint_generation().await?;
    Ok((
        store,
        OwnerAdmissionBinding {
            secure: None,
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

pub(crate) fn admission(
    binding: OwnerAdmissionBinding,
) -> Result<OwnerAdmission, crate::PersistenceError> {
    OwnerAdmission::verified(binding, Utc::now().timestamp() + 300)
}

pub(crate) fn description() -> Result<OwnerDeviceDescription, crate::PersistenceError> {
    OwnerDeviceDescription::verified("Chrome · macOS".into(), "Chrome".into(), "macOS".into())
}

#[tokio::test]
async fn connected_owner_has_no_clock_expiry_and_disconnected_entry_cannot_replay()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let entry = admission(binding.clone())?;
    let first = store.create_owner_session(&entry, &description()?).await?;
    let replay = store.create_owner_session(&entry, &description()?).await?;
    assert_eq!(first.session_bearer, replay.session_bearer);
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
    let owner = ServerOwnerAuthority::CentralSession(Box::new(first.authorization.clone()));
    assert!(
        store
            .list_room_directory_for_owner(&owner, true)
            .await?
            .1
            .is_empty()
    );
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
    // Moving the old entry deadline into the past cannot expire connected authority.
    sqlx::query("UPDATE host_owner_sessions SET admission_expires_at = ?")
        .bind(Utc::now().timestamp() - 1)
        .execute(&store.pool)
        .await?;
    let expected = RoomSessionAuthorization::Operator(issued.authorization);
    store
        .revalidate_room_session_authorization(&expected)
        .await?;
    let rows = store.owner_device_sessions(&owner).await?;
    assert_eq!(rows.len(), 1);
    assert!(rows[0].current);
    assert_eq!(rows[0].device_name, "Chrome · macOS");
    assert!(rows[0].last_connected_at.is_some());
    store
        .disconnect_owner_session(first.authorization.fingerprint())
        .await?;
    assert!(
        store
            .revalidate_room_session_authorization(&expected)
            .await
            .is_err()
    );
    // This particular consumed entry is still retained until its original short window.
    assert!(
        store
            .create_owner_session(&entry, &description()?)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn remote_all_is_account_scoped_and_native_remains_recovery_authority()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let first = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let mut same = binding.clone();
    same.entry_fingerprint = [2; 32];
    same.browser_fingerprint = [43; 32];
    let second = store
        .create_owner_session(&admission(same)?, &description()?)
        .await?;
    let mut transferred = binding;
    transferred.entry_fingerprint = [3; 32];
    transferred.person_id = "new-owner".into();
    let next = store
        .create_owner_session(&admission(transferred)?, &description()?)
        .await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(first.authorization));
    assert_eq!(store.owner_device_sessions(&owner).await?.len(), 2);
    assert!(
        store
            .revoke_owner_devices(&owner, Some(next.authorization.session_id()))
            .await
            .is_err()
    );
    let committed = store.revoke_owner_devices(&owner, None).await?;
    assert_eq!(committed.revoked_count, 2);
    assert!(
        committed
            .owner_fingerprints
            .contains(second.authorization.fingerprint())
    );
    let next_owner = ServerOwnerAuthority::CentralSession(Box::new(next.authorization));
    store.validate_server_owner(&next_owner).await?;
    assert!(store.validate_server_owner(&owner).await.is_err());
    assert_eq!(
        store
            .owner_device_sessions(&ServerOwnerAuthority::LocalOperator)
            .await?
            .len(),
        1
    );
    store
        .revoke_owner_devices(&ServerOwnerAuthority::LocalOperator, None)
        .await?;
    store
        .validate_server_owner(&ServerOwnerAuthority::LocalOperator)
        .await?;
    assert!(store.validate_server_owner(&next_owner).await.is_err());
    Ok(())
}

#[tokio::test]
async fn separately_paired_device_survives_disconnect_but_not_issuer_revocation()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let issued = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(issued.authorization.clone()));
    let room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let direct = store
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
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Operator(Box::new(direct.authorization)),
            &[7; 32],
            &binding.origin,
            Utc::now(),
        )
        .await?;
    let paired = store
        .redeem_operator_pairing(&[7; 32], &[8; 32], &binding.origin, Utc::now())
        .await?;
    assert!(paired.authorization.expires_at().is_some());
    assert!(
        store
            .redeem_operator_pairing(
                &[7; 32],
                &[8; 32],
                &binding.origin,
                Utc::now() + Duration::hours(1)
            )
            .await
            .is_err()
    );
    store
        .record_operator_connection(&paired.authorization, Some(&description()?))
        .await?;
    let session = RoomSessionAuthorization::Operator(paired.authorization);
    let attendee = admit_companion(&store, &session).await?;
    store
        .disconnect_owner_session(issued.authorization.fingerprint())
        .await?;
    store
        .revalidate_room_session_authorization(&session)
        .await?;
    store
        .revalidate_attendee_session(&attendee.authorization, Utc::now())
        .await?;
    let committed = store
        .revoke_owner_devices(
            &ServerOwnerAuthority::LocalOperator,
            Some(issued.authorization.session_id()),
        )
        .await?;
    assert_eq!(committed.revoked_count, 2);
    assert!(
        committed
            .room_sessions
            .iter()
            .any(|(_, fp)| fp == session.session_fingerprint())
    );
    assert!(
        committed
            .room_sessions
            .iter()
            .any(|(_, fp)| fp == attendee.authorization.session_fingerprint())
    );
    assert!(
        store
            .revalidate_room_session_authorization(&session)
            .await
            .is_err()
    );
    assert!(
        store
            .revalidate_attendee_session(&attendee.authorization, Utc::now())
            .await
            .is_err()
    );
    assert!(
        store
            .redeem_operator_pairing(&[7; 32], &[8; 32], &binding.origin, Utc::now())
            .await
            .is_err()
    );
    Ok(())
}

pub(crate) async fn admit_companion(
    store: &SqliteStore,
    session: &RoomSessionAuthorization,
) -> Result<crate::AttendeeAdmission, crate::PersistenceError> {
    let invite = store
        .create_companion_attendee_invite(
            session,
            crate::CompanionInviteRequest {
                request_id: uuid::Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Companion",
            },
            Utc::now(),
        )
        .await?;
    let fingerprint: [u8; 32] = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    store
        .admit_attendee(
            crate::AttendeeAdmissionRequest {
                invite_fingerprint: &fingerprint,
                client_fingerprint: &[9; 32],
                request_id: uuid::Uuid::new_v4(),
                provider_kind: "codex_live_session",
                display_name: "Companion",
            },
            Utc::now(),
        )
        .await
}

#[tokio::test]
async fn failed_revocation_does_not_report_committed_closure_and_restart_ends_custody()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let entry = admission(binding)?;
    let issued = store.create_owner_session(&entry, &description()?).await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(issued.authorization.clone()));
    sqlx::query("CREATE TRIGGER reject_revoke BEFORE UPDATE OF revoked ON host_owner_sessions BEGIN SELECT RAISE(ABORT, 'write failed'); END")
        .execute(&store.pool).await?;
    assert!(
        store
            .revoke_owner_devices(&ServerOwnerAuthority::LocalOperator, None)
            .await
            .is_err()
    );
    store.validate_server_owner(&owner).await?;
    sqlx::query("DROP TRIGGER reject_revoke")
        .execute(&store.pool)
        .await?;
    store.disconnect_all_owner_sessions().await?;
    assert!(store.validate_server_owner(&owner).await.is_err());
    assert!(
        store
            .create_owner_session(&entry, &description()?)
            .await
            .is_err()
    );
    let now = Utc::now().timestamp();
    assert!(
        OwnerAdmission::verified(entry.binding, now + Duration::minutes(6).num_seconds()).is_err()
    );
    assert!(
        OwnerDeviceDescription::verified("unsafe\nname".into(), String::new(), String::new())
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn publication_recovery_generation_only_gates_new_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, binding) = setup().await?;
    let admitted = store
        .create_owner_session(&admission(binding.clone())?, &description()?)
        .await?;
    let owner = ServerOwnerAuthority::CentralSession(Box::new(admitted.authorization.clone()));
    let room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "room", "Room")
        .await?;
    let direct = store
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
    let room_session = RoomSessionAuthorization::Operator(direct.authorization);
    // Publication recovery after a >600s outage advances this durable generation.
    // It is discovery/admission state, not a revocation of an admitted workspace.
    let generation = store.next_central_endpoint_generation().await?;
    assert!(generation > binding.generation);
    store
        .authorize_owner_session(
            admitted.authorization.fingerprint(),
            &[42; 32],
            &binding.origin,
        )
        .await?;
    store.validate_server_owner(&owner).await?;
    store
        .revalidate_room_session_authorization(&room_session)
        .await?;
    assert_eq!(store.owner_device_sessions(&owner).await?.len(), 1);
    let mut next = binding;
    next.entry_fingerprint = [3; 32];
    assert!(
        store
            .create_owner_session(&admission(next.clone())?, &description()?)
            .await
            .is_err()
    );
    next.generation = generation;
    store
        .create_owner_session(&admission(next)?, &description()?)
        .await?;
    store
        .revoke_owner_devices(&owner, Some(admitted.authorization.session_id()))
        .await?;
    assert!(store.validate_server_owner(&owner).await.is_err());
    assert!(
        store
            .revalidate_room_session_authorization(&room_session)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn secure_owner_retry_and_child_transport_require_the_same_channel()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, mut binding) = setup().await?;
    let secure = crate::SecureSessionBinding {
        client_key_fingerprint: [7; 32],
        channel_id: "a".repeat(43),
    };
    binding.secure = Some(secure.clone());
    let entry = admission(binding.clone())?;
    let first = store.create_owner_session(&entry, &description()?).await?;
    let exact = store.create_owner_session(&entry, &description()?).await?;
    assert_eq!(first.session_bearer, exact.session_bearer);
    binding.secure.as_mut().ok_or("missing binding")?.channel_id = "b".repeat(43);
    assert!(
        store
            .create_owner_session(&admission(binding.clone())?, &description()?)
            .await
            .is_err()
    );
    let room = store
        .create_room_for_local_operator(&uuid::Uuid::new_v4().to_string(), "secure-room", "Room")
        .await?;
    let child = store
        .create_central_owner_session(
            &ServerOwnerAuthority::CentralSession(Box::new(first.authorization.clone())),
            &CentralOwnerSessionRequest::host_owned(
                "secure-room",
                room.room.room_uid,
                &[17; 32],
                &[42; 32],
                &binding.origin,
                Utc::now(),
            ),
        )
        .await?;
    let room_session = RoomSessionAuthorization::Operator(child.authorization);
    store
        .require_secure_room_transport(&room_session, Some(&secure))
        .await?;
    assert!(
        store
            .require_secure_room_transport(&room_session, None)
            .await
            .is_err()
    );
    assert!(
        store
            .require_secure_room_transport(&room_session, binding.secure.as_ref())
            .await
            .is_err()
    );
    store.disconnect_secure_channel(&secure.channel_id).await?;
    assert!(
        store
            .revalidate_room_session_authorization(&room_session)
            .await
            .is_err()
    );
    assert!(
        store
            .create_owner_session(&entry, &description()?)
            .await
            .is_err()
    );
    Ok(())
}
