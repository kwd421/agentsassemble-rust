use super::*;
use crate::MintedHumanInvite;
use crate::{
    CentralOwnerSessionRequest, OperatorSessionAuthorization, RoomManagerAuthority,
    ServerOwnerAuthority,
};
use agentsassemble_domain::{
    AuthenticatedPrincipal, ConfirmationDecision, ConfirmationState, ConfirmedRoomAction,
    DecideDelegatedAction, LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, ParticipantRole,
    RequestCreateInvite,
};

const ORIGIN: &str = "https://room.example.test";

/// An ordinary paired operator browser: same local-operator participant, but no
/// central-owner provenance.
async fn ordinary_operator(
    store: &SqliteStore,
) -> Result<OperatorSessionAuthorization, Box<dyn std::error::Error>> {
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let now = chrono::Utc::now();
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Local(manager),
            &[0x41; 32],
            ORIGIN,
            now,
        )
        .await?;
    Ok(store
        .redeem_operator_pairing(&[0x41; 32], &[0x42; 32], ORIGIN, now)
        .await?
        .authorization)
}

async fn central_owner_operator(
    store: &SqliteStore,
) -> Result<OperatorSessionAuthorization, Box<dyn std::error::Error>> {
    let manager = store
        .authorize_local_room_manager(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let now = chrono::Utc::now();
    let request = CentralOwnerSessionRequest::new(
        "general",
        manager.room_uid,
        &[0x43; 32],
        &[0x44; 32],
        ORIGIN,
        now + chrono::Duration::minutes(4),
        now,
    );
    Ok(store
        .create_central_owner_session(&ServerOwnerAuthority::LocalOperator, &request)
        .await?
        .authorization)
}

async fn pending_invite(
    store: &SqliteStore,
    delegate: &DelegateRoomAuthority,
) -> Result<String, Box<dyn std::error::Error>> {
    let pending = store
        .request_delegated_human_invite(delegate, &params(), |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    Ok(pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned())
}

fn params() -> RequestCreateInvite {
    RequestCreateInvite {
        request_id: Uuid::new_v4().to_string(),
        display_name: Some("Guest Label".into()),
        invite_scope: "read_only".into(),
        ttl_seconds: 600,
        max_uses: 3,
    }
}

fn params_with(change: impl FnOnce(&mut RequestCreateInvite)) -> RequestCreateInvite {
    let mut params = params();
    change(&mut params);
    params
}

/// The local room owner, the bound director delegate and one existing invite fixture.
async fn invite_fixture()
-> Result<(SqliteStore, AuthenticatedPrincipal, DelegateRoomAuthority), Box<dyn std::error::Error>>
{
    let (store, _) =
        crate::human_session_authority_tests::admitted_fixture(InviteScope::ReadWrite).await;
    let owner = crate::human_session_authority_tests::local_operator_principal();
    let delegate = director(&store).await?;
    Ok((store, owner, delegate))
}

fn minted() -> Result<MintedHumanInvite, Box<dyn std::error::Error>> {
    let created_at = chrono::DateTime::from_timestamp_micros(chrono::Utc::now().timestamp_micros())
        .ok_or("time")?;
    Ok(MintedHumanInvite {
        signed_token_fingerprint: [0xAB; 32],
        join_code_fingerprint: [0xCD; 32],
        base_participant_id: "guest-invite-test".into(),
        created_at,
        expires_at: created_at + chrono::Duration::seconds(600),
    })
}

async fn invite_rows(store: &SqliteStore) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_invites")
            .fetch_one(&store.pool)
            .await?,
    )
}

async fn request_rows(store: &SqliteStore) -> Result<i64, Box<dyn std::error::Error>> {
    Ok(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delegated_action_requests")
            .fetch_one(&store.pool)
            .await?,
    )
}

#[tokio::test]
async fn unconfirmed_invite_binds_policy_and_only_the_owner_approves() -> TestResult {
    let (store, owner, delegate) = invite_fixture().await?;
    let baseline = invite_rows(&store).await?;
    let params = params();
    let pending = store
        .request_delegated_human_invite(&delegate, &params, |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let id = pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned();
    // Nothing is minted before approval: no new invite row and no stored credential.
    assert_eq!(invite_rows(&store).await?, baseline);
    let (request, _, _) = store.approved_invite_snapshot("general", &id).await?;
    let ConfirmedRoomAction::CreateHumanInvite(bound) = &request.action else {
        return Err("bound invite missing".into());
    };
    assert_eq!(bound.params, params);
    assert_eq!(request.target_name, "Guest Label");
    assert!(!serde_json::to_string(&request)?.contains("token"));
    // Exact retries return the same receipt without a second debit.
    let retry = store
        .request_delegated_human_invite::<()>(&delegate, &params, |_, _, _| panic!("retry charged"))
        .await?
        .0;
    assert_eq!(pending, retry);
    // Changed parameters under the same identity conflict instead.
    let mut changed = params.clone();
    changed.max_uses = 9;
    assert!(
        matches!(store.request_delegated_human_invite::<()>(&delegate, &changed, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "request_conflict")
    );
    // The delegate itself cannot approve its own request.
    let mut impostor = owner.clone();
    impostor.participant_id = delegate.delegate_id().to_owned();
    let decision = DecideDelegatedAction {
        confirmation_id: id.clone(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    assert!(
        matches!(store.decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&impostor), &decision).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
    );
    // Approval reserves the execution only: still no invite row and no credential in the ACK.
    let ack = store
        .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
        .await?;
    assert_eq!(ack.result["state"], "executing", "{:?}", ack.result);
    assert!(ack.result.get("invite").is_none());
    assert_eq!(invite_rows(&store).await?, baseline);
    // A replayed decision returns the receipt without the credential or link.
    let replay = store
        .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
        .await?;
    assert!(replay.deduplicated);
    assert!(replay.result.get("invite").is_none());
    // The drive mints through the owner's own core, binding the stored policy exactly.
    let minted = minted()?;
    let drive = store
        .drive_approved_human_invite("general", &id, &minted)
        .await?;
    let invite = drive.invite.clone().ok_or("minted invite missing")?;
    assert_eq!(invite.invite_id, "abababababababab");
    assert_eq!(invite.room_id, "general");
    assert_eq!(invite.display_name, "Guest Label");
    assert_eq!(invite.invite_scope, InviteScope::ReadOnly);
    assert_eq!(invite.max_uses, 3);
    assert_eq!(invite.created_by_user_id, owner.principal_id);
    assert_eq!(
        store
            .human_invite_by_signed_fingerprint(&minted.signed_token_fingerprint)
            .await?
            .ok_or("stored invite row")?,
        invite
    );
    assert_eq!(drive.state, ConfirmationState::Succeeded);
    let receipt = &drive.receipt;
    assert_eq!(receipt["status"], "succeeded");
    assert_eq!(receipt["action"], "create_human_invite");
    assert!(receipt.get("target_id").is_none());
    for forbidden in ["credential", "bearer", "token", "secret", "password"] {
        assert!(
            !serde_json::to_string(receipt)?.contains(forbidden),
            "receipt carries {forbidden}: {receipt}"
        );
    }
    // The system line names the AI acting for the owner and carries the confirmation id.
    let attributed: Vec<_> = store
        .snapshot("general", 0, 400)
        .await?
        .events
        .into_iter()
        .filter(|event| event.extra.get("confirmation_id") == Some(&json!(id)))
        .collect();
    assert!(attributed.iter().any(|event| {
        event.actor.participant_id == delegate.delegate_id()
            && event.message_kind.as_deref() == Some("system")
            && event
                .content
                .as_ref()
                .is_some_and(|text| text.contains("on behalf of"))
    }));
    for event in &attributed {
        let encoded = serde_json::to_string(event)?;
        for forbidden in ["aai1.", "aaj1_"] {
            assert!(
                !encoded.contains(forbidden),
                "room event carries {forbidden}: {encoded}"
            );
        }
    }
    // A second drive returns only the recorded receipt, never another credential.
    let again = store
        .drive_approved_human_invite("general", &id, &minted)
        .await?;
    assert!(again.invite.is_none());
    assert_eq!(again.receipt, *receipt);
    assert_eq!(invite_rows(&store).await?, baseline + 1);
    Ok(())
}

#[tokio::test]
async fn invite_refuses_invalid_policy_and_an_unsupported_owner_provenance() -> TestResult {
    let (store, _owner, delegate) = invite_fixture().await?;
    let baseline = invite_rows(&store).await?;
    for bad in [
        params_with(|p| p.ttl_seconds = 0),
        params_with(|p| p.max_uses = -1),
        params_with(|p| p.invite_scope = "everyone".into()),
        params_with(|p| p.display_name = Some("line\nbreak".into())),
    ] {
        assert!(
            matches!(store.request_delegated_human_invite::<()>(&delegate, &bad, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "invalid_human_invite")
        );
    }
    assert_eq!(invite_rows(&store).await?, baseline);
    assert_eq!(request_rows(&store).await?, 0);
    // A lifetime the owner's own route cannot represent as a checked duration is
    // refused with the owner route's error before anything is queued.
    for over_limit in [i64::MAX, 9_223_372_036_854_776] {
        assert!(
            matches!(store.request_delegated_human_invite::<()>(&delegate, &params_with(|p| p.ttl_seconds = over_limit), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "bad_request")
        );
    }
    assert_eq!(invite_rows(&store).await?, baseline);
    assert_eq!(request_rows(&store).await?, 0);
    // A large but representable lifetime still queues.
    let (queued, _) = store
        .request_delegated_human_invite::<()>(
            &delegate,
            &params_with(|p| p.ttl_seconds = 86_400 * 30),
            |_, _, _| Ok(()),
        )
        .await?;
    assert_eq!(queued["status"], "pending_confirmation");
    // Companion, attendee and connector invites have no schema: unknown fields are refused.
    let unknown = json!({
        "request_id": Uuid::new_v4().to_string(),
        "invite_scope": "read_write",
        "ttl_seconds": 600,
        "max_uses": 1,
        "kind": "companion",
    });
    assert!(serde_json::from_value::<RequestCreateInvite>(unknown).is_err());
    // A human guest/member provenance cannot become the owner's manager authority.
    let fp = crate::human_session_authority_tests::session_fingerprint(&store).await;
    let human = store.authorize_human_session(&fp).await?;
    let guest = human.principal().clone();
    let mut participant = store.participant("general", &guest.participant_id).await?;
    participant.participant_id = "human-delegate".into();
    participant.owner_id = guest.participant_id.clone();
    participant.participant_type = "agent".into();
    participant.role = ParticipantRole::Director;
    let mut tx = store.pool.begin().await?;
    sqlx::query("INSERT INTO participants(room_id,participant_id,participant_json) VALUES(?,?,?)")
        .bind(&participant.room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut *tx)
        .await?;
    crate::delegation_owner::OwnerBinding::from_mutation(
        &mut tx,
        RoomMutationAuthority::HumanSession(&human),
    )
    .await?
    .save(&mut tx, "general", "participant", "human-delegate")
    .await?;
    assert!(matches!(
        crate::delegation_owner::manager_authority_for_delegate(
            &mut tx,
            "general",
            "human-delegate"
        )
        .await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_authority_unavailable"
    ));
    tx.rollback().await?;
    Ok(())
}

#[tokio::test]
async fn a_guest_ai_cannot_request_a_human_invite() -> TestResult {
    let (store, session, now) = crate::attendee::connection_tests::fixture().await?;
    let baseline = invite_rows(&store).await?;
    let connection = store
        .claim_attendee_connection(&session, Uuid::new_v4(), None, now)
        .await?
        .authorization;
    store
        .record_attendee_ready(&connection, &crate::attendee::ready_tests::report(), now)
        .await?;
    let fingerprint = crate::human_session_authority_tests::session_fingerprint(&store).await;
    let human = store.authorize_human_session(&fingerprint).await?;
    store
        .execute_authorized_message_with_turn(
            RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content": "Guest invite"}),
        )
        .await?;
    let turn = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn missing")?;
    let local = crate::human_session_authority_tests::local_operator_principal();
    store
        .execute_participant_role_update(
            RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"participant_id":session.principal().participant_id,"role":"director"}),
        )
        .await?;
    let authority = DelegateRoomAuthority::Attendee {
        session,
        connection_id: connection.connection_id(),
        turn_generation: turn.authority.turn_generation,
        execution_id: turn.authority.execution_id,
    };
    // The guest owner cannot create a human invite, so their AI is refused rather than queued.
    assert!(
        matches!(store.request_delegated_human_invite::<()>(&authority, &params(), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_not_permitted")
    );
    assert_eq!(
        store.delegated_direct_actions(&authority).await?,
        vec!["set_message_pin"]
    );
    assert_eq!(request_rows(&store).await?, 0);
    assert_eq!(invite_rows(&store).await?, baseline);
    Ok(())
}

#[tokio::test]
async fn an_ordinary_paired_operator_cannot_approve_a_human_invite() -> TestResult {
    let (store, _owner, delegate) = invite_fixture().await?;
    let baseline = invite_rows(&store).await?;
    let id = pending_invite(&store, &delegate).await?;
    let ordinary = ordinary_operator(&store).await?;
    assert!(!ordinary.is_central_owner());
    // The same local-operator participant, but an ordinary paired browser: approval
    // is refused and nothing is minted or stored.
    let approve = DecideDelegatedAction {
        confirmation_id: id.clone(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    assert!(
        matches!(store.decide_delegated_action(RoomMutationAuthority::OperatorSession(&ordinary), &approve).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
    );
    assert_eq!(invite_rows(&store).await?, baseline);
    let (request, _, receipt) = store.approved_invite_snapshot("general", &id).await?;
    assert!(matches!(request.state, ConfirmationState::Pending));
    assert_eq!(receipt["status"], "pending_confirmation");
    // Decline stays allowed for the same session.
    let ack = store
        .decide_delegated_action(
            RoomMutationAuthority::OperatorSession(&ordinary),
            &DecideDelegatedAction {
                confirmation_id: id,
                decision: ConfirmationDecision::Decline,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(ack.result["state"], "declined");
    assert!(ack.result.get("invite").is_none());
    Ok(())
}

#[tokio::test]
async fn a_central_owner_session_approves_and_the_mint_binds_that_session() -> TestResult {
    let (store, _owner, delegate) = invite_fixture().await?;
    let baseline = invite_rows(&store).await?;
    let id = pending_invite(&store, &delegate).await?;
    let central = central_owner_operator(&store).await?;
    assert!(central.is_central_owner());
    let ack = store
        .decide_delegated_action(
            RoomMutationAuthority::OperatorSession(&central),
            &DecideDelegatedAction {
                confirmation_id: id.clone(),
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(ack.result["state"], "executing", "{:?}", ack.result);
    assert!(ack.result.get("invite").is_none());
    assert_eq!(invite_rows(&store).await?, baseline);
    let drive = store
        .drive_approved_human_invite("general", &id, &minted()?)
        .await?;
    assert!(drive.invite.is_some());
    assert_eq!(drive.state, ConfirmationState::Succeeded);
    assert_eq!(invite_rows(&store).await?, baseline + 1);
    Ok(())
}

#[tokio::test]
async fn a_session_revoked_after_approval_mints_no_invite() -> TestResult {
    let (store, _owner, delegate) = invite_fixture().await?;
    let baseline = invite_rows(&store).await?;
    let id = pending_invite(&store, &delegate).await?;
    let central = central_owner_operator(&store).await?;
    let ack = store
        .decide_delegated_action(
            RoomMutationAuthority::OperatorSession(&central),
            &DecideDelegatedAction {
                confirmation_id: id.clone(),
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(ack.result["state"], "executing", "{:?}", ack.result);
    sqlx::query("UPDATE operator_pairings SET revoked = 1")
        .execute(&store.pool)
        .await?;
    let drive = store
        .drive_approved_human_invite("general", &id, &minted()?)
        .await?;
    assert!(drive.invite.is_none());
    assert_eq!(drive.state, ConfirmationState::Cancelled);
    assert_eq!(drive.receipt["code"], "confirmation_cancelled");
    assert_eq!(invite_rows(&store).await?, baseline);
    Ok(())
}

#[tokio::test]
async fn manager_authority_refuses_an_ordinary_operator_provenance() -> TestResult {
    let (store, _owner, _delegate) = invite_fixture().await?;
    let ordinary = ordinary_operator(&store).await?;
    let mut tx = store.pool.begin().await?;
    crate::delegation_owner::OwnerBinding::from_mutation(
        &mut tx,
        RoomMutationAuthority::OperatorSession(&ordinary),
    )
    .await?
    .save(&mut tx, "general", "participant", "operator-delegate")
    .await?;
    assert!(matches!(
        crate::delegation_owner::manager_authority_for_delegate(
            &mut tx,
            "general",
            "operator-delegate"
        )
        .await,
        Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_authority_unavailable"
    ));
    tx.rollback().await?;
    Ok(())
}
