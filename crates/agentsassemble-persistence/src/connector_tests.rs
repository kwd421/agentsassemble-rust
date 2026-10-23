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

    assert_terminal_room_leave_receipt(&store, authority, &first.outcome).await?;

    Ok(())
}

async fn assert_terminal_room_leave_receipt(
    store: &crate::SqliteStore,
    authority: &crate::ConnectorSessionAuthorization,
    original: &crate::CommandOutcome,
) -> TestResult {
    // Losing the leave reply does not make its committed receipt an active-room operation.
    let local = crate::room_user_identity::test_authority(store).await;
    let mut tx = store.pool.begin().await?;
    let principal = local.resolve(&mut tx).await?;
    tx.commit().await?;
    for (action, payload) in [
        (
            "room.archive",
            serde_json::json!({"room_uid":local.room_uid,"archived":true}),
        ),
        ("room.close", serde_json::json!({"room_uid":local.room_uid})),
    ] {
        store
            .execute_room_lifecycle(
                crate::RoomMutationAuthority::TrustedPrincipal(&principal),
                action,
                action,
                &payload,
            )
            .await?;
        let recovered = store
            .completed_connector_leave(
                authority.session_fingerprint(),
                "same-leave",
                &serde_json::json!({}),
            )
            .await?
            .ok_or("terminal room lost its committed leave receipt")?;
        assert_eq!(recovered.1.result, original.result);
        assert!(recovered.1.deduplicated);
        assert!(
            store
                .authorize_connector_session(authority.session_fingerprint(), Utc::now())
                .await
                .is_err()
        );
    }

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
    assert!(removed.cleanups.is_empty());
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
    assert!(exported.cleanups.is_empty());
    Ok(())
}

#[tokio::test]
async fn a_connected_ai_reply_takes_discords_shape() -> TestResult {
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[5; 32], Uuid::new_v4(), "Reply AI", now)
        .await?;
    let local = crate::room_user_identity::test_authority(&store).await;
    let mut tx = store.pool.begin().await?;
    let host = local.resolve(&mut tx).await?;
    tx.commit().await?;
    let source = store
        .execute_message_with_turn(
            &host,
            "source",
            "message.send",
            &serde_json::json!({"content":"question"}),
        )
        .await?;
    let reply = store
        .execute_authorized_message_with_turn(
            crate::RoomMutationAuthority::ConnectorSession(&admitted.authorization),
            "reply",
            "message.send",
            &serde_json::json!({
                "content": format!("&lt;<@{}>&gt; answer", host.participant_id),
                "reply_to_event_id": source.outcome.event.id,
            }),
        )
        .await?;
    assert_eq!(reply.outcome.event.content.as_deref(), Some("answer"));
    assert_eq!(
        reply.outcome.event.reply_mention(),
        Some(host.participant_id.as_str())
    );
    Ok(())
}

#[tokio::test]
async fn a_connector_in_use_is_renewed_and_an_unused_one_ends() -> TestResult {
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[1; 32], Uuid::new_v4(), "Kept", now)
        .await?;
    let session = *admitted.authorization.session_fingerprint();
    // Early use leaves the expiry alone.
    let early = store
        .authorize_connector_session(&session, now + Duration::minutes(10))
        .await?;
    assert_eq!(early.expires_at(), admitted.authorization.expires_at());
    // Past half its lifetime, use pushes the expiry a full hour out.
    let later = now + Duration::minutes(40);
    let renewed = store.authorize_connector_session(&session, later).await?;
    assert!(renewed.expires_at() > admitted.authorization.expires_at());
    // Authority issued before the renewal stays valid.
    store
        .revalidate_connector_session(&admitted.authorization, later)
        .await?;
    // It outlives the first hour while in use, and ends once unused.
    store
        .authorize_connector_session(&session, now + Duration::minutes(90))
        .await?;
    assert!(
        store
            .authorize_connector_session(&session, now + Duration::hours(4))
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn a_seat_invitation_takes_the_same_seat_back() -> TestResult {
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let first = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let first_fingerprint = Sha256::digest(first.invite_bearer.as_bytes()).into();
    let seated = store
        .admit_connector(&first_fingerprint, &[1; 32], Uuid::new_v4(), "Ace", now)
        .await?;
    let seat = seated.authorization.principal().participant_id.clone();
    let old_session = *seated.authorization.session_fingerprint();
    // A person cannot be a seat.
    assert!(
        store
            .create_connector_seat_invite(
                &manager,
                Uuid::new_v4(),
                InviteScope::ReadWrite,
                "operator-local",
                now
            )
            .await
            .is_err()
    );
    let again = store
        .create_connector_seat_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, &seat, now)
        .await?;
    let again_fingerprint = Sha256::digest(again.invite_bearer.as_bytes()).into();
    let back = store
        .admit_connector(
            &again_fingerprint,
            &[2; 32],
            Uuid::new_v4(),
            "Another name",
            now,
        )
        .await?;
    assert_eq!(back.authorization.principal().participant_id, seat);
    assert_eq!(back.authorization.principal().display_name, "Ace");
    assert_eq!(back.replaced_sessions, vec![old_session]);
    assert!(
        store
            .authorize_connector_session(&old_session, now)
            .await
            .is_err()
    );
    store
        .authorize_connector_session(back.authorization.session_fingerprint(), now)
        .await?;
    let agents = store
        .snapshot("general", 0, 200)
        .await?
        .participants
        .into_iter()
        .filter(|participant| participant.participant_type == "agent")
        .count();
    assert_eq!(agents, 1, "no second seat");
    Ok(())
}

#[tokio::test]
async fn an_away_external_ai_stays_listed_as_detached_until_it_can_no_longer_return() -> TestResult
{
    let (store, manager) = fixture().await?;
    let now = Utc::now();
    let invite = store
        .create_connector_invite(&manager, Uuid::new_v4(), InviteScope::ReadWrite, now)
        .await?;
    let fingerprint = Sha256::digest(invite.invite_bearer.as_bytes()).into();
    let admitted = store
        .admit_connector(&fingerprint, &[1; 32], Uuid::new_v4(), "Away", now)
        .await?;
    let seat = admitted.authorization.principal().participant_id.clone();
    let owner = agentsassemble_domain::AuthenticatedPrincipal {
        principal_id: agentsassemble_domain::LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: agentsassemble_domain::CapabilitySet::local_operator(
            ClientKind::Browser,
            InviteScope::ReadWrite,
        ),
    };
    let roster = || {
        store.snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&owner),
            0,
            200,
        )
    };
    let status = |participants: Vec<agentsassemble_domain::Participant>| {
        participants
            .into_iter()
            .find(|participant| participant.participant_id == seat)
            .map(|participant| participant.status)
    };
    assert_eq!(
        status(roster().await?.participants),
        Some(agentsassemble_domain::ParticipantStatus::Joined)
    );
    for (ago, expected) in [
        (
            Duration::hours(2),
            Some(agentsassemble_domain::ParticipantStatus::Detached),
        ),
        (Duration::days(8), None),
    ] {
        sqlx::query(
            "UPDATE room_connector_invites SET session_expires_at = ? WHERE participant_id = ?",
        )
        .bind((now - ago).timestamp_micros())
        .bind(&seat)
        .execute(&store.pool)
        .await?;
        assert_eq!(status(roster().await?.participants), expected);
    }
    Ok(())
}
