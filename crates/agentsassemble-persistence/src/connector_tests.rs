use agentsassemble_domain::{ClientKind, InviteScope};
use chrono::{Duration, Utc};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::{PersistenceError, RoomManagerAuthority};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn connector_one_use_retry_and_leave_have_one_agent_owner() -> TestResult {
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let request = Uuid::new_v4();
    let invite = store
        .create_connector_invite(&manager, request, InviteScope::ReadWrite, now)
        .await?;
    let repeat = store
        .create_connector_invite(
            &manager,
            request,
            InviteScope::ReadWrite,
            now + Duration::seconds(1),
        )
        .await?;
    assert_eq!(invite.invite_id, repeat.invite_id);
    assert_eq!(invite.invite_bearer, repeat.invite_bearer);
    assert_eq!(invite.expires_at, repeat.expires_at);
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let join_id = Uuid::new_v4();
    let (first, other) = tokio::join!(
        store.admit_connector(&fingerprint, &[1; 32], join_id, "External AI", now),
        store.admit_connector(&fingerprint, &[2; 32], join_id, "External AI", now),
    );
    let (winner, client) = match (first, other) {
        (Ok(admitted), Err(_)) => (admitted, [1; 32]),
        (Err(_), Ok(admitted)) => (admitted, [2; 32]),
        _ => panic!("one invite must have exactly one owner"),
    };
    let principal = winner.authorization.principal();
    assert_eq!(principal.client_kind, ClientKind::RoomConnector);
    assert!(!principal.is_operator);
    assert!(principal.capabilities.message_send);
    assert!(!principal.capabilities.room_manage);
    let replay = store
        .admit_connector(
            &fingerprint,
            &client,
            join_id,
            "External AI",
            now + Duration::seconds(2),
        )
        .await?;
    assert!(replay.deduplicated);
    assert_eq!(replay.event.id, winner.event.id);
    assert_eq!(replay.session_bearer, winner.session_bearer);
    assert_eq!(
        replay.authorization.expires_at(),
        winner.authorization.expires_at()
    );
    assert!(matches!(
        store
            .admit_connector(&fingerprint, &client, join_id, "Changed name", now)
            .await,
        Err(PersistenceError::CommandRejected { code, .. }) if matches!(code.as_bytes(), b"invite_already_used")
    ));
    assert_eq!(
        store
            .snapshot("general", 0, 100)
            .await?
            .participants
            .iter()
            .filter(|p| p.participant_type == "agent")
            .count(),
        1
    );
    let leave = store
        .leave_connector_session(&winner.authorization, "connector-leave")
        .await?;
    assert_eq!(leave.outcome.event.actor.participant_type, "agent");
    assert!(
        store
            .revalidate_connector_session(&winner.authorization, now)
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn connector_expiry_archive_and_creation_replay_never_restore_authority() -> TestResult {
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let creation = Uuid::new_v4();
    let invite = store
        .create_connector_invite(&manager, creation, InviteScope::ReadOnly, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[3; 32], Uuid::new_v4(), "Reader AI", now)
        .await?;
    assert_eq!(
        admitted.event.extra["participant"]["owner_id"],
        agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
    );
    assert!(!admitted.authorization.principal().capabilities.message_send);
    assert!(admitted.authorization.principal().capabilities.room_history);
    assert!(
        store
            .revalidate_connector_session(&admitted.authorization, now + Duration::hours(1))
            .await
            .is_err()
    );
    store
        .create_connector_invite(
            &manager,
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            now + Duration::hours(2),
        )
        .await?;
    assert!(
        store
            .create_connector_invite(
                &manager,
                creation,
                InviteScope::ReadOnly,
                now + Duration::hours(2)
            )
            .await
            .is_err()
    );
    let local = crate::room_user_identity::test_authority(&store).await;
    let mut tx = store.pool.begin().await?;
    let principal = local.resolve(&mut tx).await?;
    tx.commit().await?;
    store
        .execute_room_lifecycle(
            crate::RoomMutationAuthority::TrustedPrincipal(&principal),
            "archive-connector",
            "room.archive",
            &serde_json::json!({"room_uid":local.room_uid,"archived":true}),
        )
        .await?;
    store
        .execute_room_lifecycle(
            crate::RoomMutationAuthority::TrustedPrincipal(&principal),
            "restore-connector",
            "room.archive",
            &serde_json::json!({"room_uid":local.room_uid,"archived":false}),
        )
        .await?;
    assert!(
        store
            .revalidate_connector_session(&admitted.authorization, now)
            .await
            .is_err()
    );
    Ok(())
}

async fn fixture() -> Result<(crate::SqliteStore, RoomManagerAuthority), Box<dyn std::error::Error>>
{
    let store = crate::SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let authority = crate::room_user_identity::test_authority(&store).await;
    Ok((store, RoomManagerAuthority::Local(authority)))
}

#[tokio::test]
async fn connector_terminal_leave_receipt_requires_exact_credential_and_incarnation() -> TestResult
{
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadOnly, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[5; 32], Uuid::new_v4(), "Leaving AI", now)
        .await?;
    let authority = &admitted.authorization;
    let (first, second) = tokio::join!(
        store.leave_connector_session(authority, "same-leave"),
        store.leave_connector_session(authority, "same-leave"),
    );
    let (first, second) = (first?, second?);
    assert_eq!(first.outcome.result, second.outcome.result);
    assert_ne!(first.outcome.deduplicated, second.outcome.deduplicated);
    assert_eq!(
        first.revoked_session_fingerprints.len() + second.revoked_session_fingerprints.len(),
        1
    );
    let recovered = store
        .completed_connector_leave(
            authority.session_fingerprint(),
            "same-leave",
            &serde_json::json!({}),
        )
        .await?
        .ok_or("receipt missing")?;
    assert_eq!(recovered.1.result, first.outcome.result);
    assert!(recovered.1.deduplicated);
    assert!(
        store
            .authorize_connector_session(authority.session_fingerprint(), now)
            .await
            .is_err()
    );
    assert!(
        store
            .leave_connector_session(authority, "different-leave")
            .await
            .is_err()
    );
    assert!(
        store
            .completed_connector_leave(
                authority.session_fingerprint(),
                "different-leave",
                &serde_json::json!({})
            )
            .await?
            .is_none()
    );
    for (credential, payload) in [
        (
            authority.session_fingerprint(),
            serde_json::json!({"changed":true}),
        ),
        (&[99; 32], serde_json::json!({})),
    ] {
        assert!(
            store
                .completed_connector_leave(credential, "same-leave", &payload)
                .await
                .is_err()
        );
    }
    assert_eq!(
        store
            .snapshot("general", 0, 200)
            .await?
            .events
            .iter()
            .filter(|event| event.event_type == "participant_left")
            .count(),
        1
    );

    // Retain the original credential and receipt while replacing the room incarnation.
    sqlx::query(
        "UPDATE rooms SET room_json=json_set(room_json,'$.room_uid',?) WHERE room_id='general'",
    )
    .bind(Uuid::new_v4().to_string())
    .execute(&store.pool)
    .await?;
    assert!(
        store
            .completed_connector_leave(
                authority.session_fingerprint(),
                "same-leave",
                &serde_json::json!({})
            )
            .await
            .is_err()
    );
    assert!(
        store
            .leave_connector_session(authority, "same-leave")
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn connector_moderation_controls_membership_without_claiming_a_provider_process() -> TestResult
{
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[4; 32], Uuid::new_v4(), "Moderated AI", now)
        .await?;
    let local = crate::room_user_identity::test_authority(&store).await;
    let mut tx = store.pool.begin().await?;
    let principal = local.resolve(&mut tx).await?;
    tx.commit().await?;
    let target = &admitted.authorization.principal().participant_id;
    let muted = store
        .execute_participant_mute(
            crate::RoomMutationAuthority::TrustedPrincipal(&principal),
            "mute-connector",
            &serde_json::json!({"participant_id":target,"muted":true}),
        )
        .await?;
    assert!(muted.host_interrupt_effect.is_none());
    assert!(muted.assignments.is_empty());
    assert!(
        store
            .execute_authorized_message_with_turn(
                crate::RoomMutationAuthority::ConnectorSession(&admitted.authorization),
                "muted-write",
                "message.send",
                &serde_json::json!({"content":"must not send"})
            )
            .await
            .is_err()
    );
    let removed = store
        .execute_participant_removal(
            crate::RoomMutationAuthority::TrustedPrincipal(&principal),
            "kick-connector",
            "participant.kick",
            &serde_json::json!({"participant_id":target}),
        )
        .await?;
    assert!(removed.cleanup.is_none());
    assert_eq!(
        removed.revoked_session_fingerprints,
        vec![*admitted.authorization.session_fingerprint()]
    );
    assert!(
        store
            .snapshot_for(
                crate::RoomMutationAuthority::ConnectorSession(&admitted.authorization),
                0,
                200
            )
            .await
            .is_err()
    );
    assert!(
        store
            .execute_authorized_message_with_turn(
                crate::RoomMutationAuthority::ConnectorSession(&admitted.authorization),
                "removed-write",
                "message.send",
                &serde_json::json!({"content":"must not send"})
            )
            .await
            .is_err()
    );
    let exported = store
        .execute_participant_removal(
            crate::RoomMutationAuthority::TrustedPrincipal(&principal),
            "export-connector",
            "participant.export",
            &serde_json::json!({"participant_id":target}),
        )
        .await?;
    assert!(exported.cleanup.is_none());
    Ok(())
}
