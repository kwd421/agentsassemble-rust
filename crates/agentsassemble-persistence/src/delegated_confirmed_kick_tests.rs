use super::*;
use crate::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
    HumanSessionAuthorization, PreparedHumanAdmission,
};
use agentsassemble_domain::{
    AuthenticatedPrincipal, ConfirmationDecision, ConfirmedRoomAction, DecideDelegatedAction,
    InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID, ParticipantStatus, RequestKickParticipant,
};

const GUEST_ID: &str = "session-guest";

fn params(participant_id: &str) -> RequestKickParticipant {
    RequestKickParticipant {
        request_id: Uuid::new_v4().to_string(),
        participant_id: participant_id.into(),
    }
}

/// The local room owner, the bound delegate and one removable human ("Session Guest").
async fn kick_fixture()
-> Result<(SqliteStore, AuthenticatedPrincipal, DelegateRoomAuthority), Box<dyn std::error::Error>>
{
    let (store, _) =
        crate::human_session_authority_tests::admitted_fixture(InviteScope::ReadWrite).await;
    let owner = crate::human_session_authority_tests::local_operator_principal();
    let delegate = director(&store).await?;
    Ok((store, owner, delegate))
}

async fn bound_kick(
    store: &SqliteStore,
    delegate: &DelegateRoomAuthority,
    participant_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let pending = store
        .request_delegated_participant_kick(delegate, &params(participant_id), |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    Ok(pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned())
}

async fn live_session_state(
    store: &SqliteStore,
    participant_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    Ok(sqlx::query_scalar(
        "SELECT state FROM human_room_sessions WHERE room_id='general' AND participant_id=?",
    )
    .bind(participant_id)
    .fetch_one(&store.pool)
    .await?)
}

#[tokio::test]
async fn unconfirmed_kick_changes_nothing_and_only_the_owner_approves() -> TestResult {
    let (store, owner, delegate) = kick_fixture().await?;
    let params = params(GUEST_ID);
    let pending = store
        .request_delegated_participant_kick(&delegate, &params, |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    // The pending request removes nobody and ends no live session.
    assert_eq!(
        store.participant("general", GUEST_ID).await?.status,
        ParticipantStatus::Joined
    );
    assert_eq!(live_session_state(&store, GUEST_ID).await?, "active");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_events WHERE room_id='general' AND json_extract(event_json,'$.request_id')=?"
        )
        .bind(&params.request_id)
        .fetch_one(&store.pool)
        .await?,
        0
    );
    // The request binds the target's current admission identity without any re-read.
    let (request, _, _) = store.approved_kick_snapshot("general", id).await?;
    let ConfirmedRoomAction::KickParticipant(bound) = &request.action else {
        return Err("bound kick missing".into());
    };
    assert_eq!(bound.participant_id, GUEST_ID);
    assert_eq!(bound.params, params);
    assert_eq!(
        bound.expected_created_at,
        store.participant("general", GUEST_ID).await?.created_at
    );
    assert_eq!(request.target_name, "Session Guest");
    assert!(request.consequences.contains("Session Guest"));
    // Exact retries return the same receipt without a second debit.
    let retry = store
        .request_delegated_participant_kick::<()>(&delegate, &params, |_, _, _| {
            panic!("retry charged")
        })
        .await?
        .0;
    assert_eq!(pending, retry);
    // Retargeting under the same request identity conflicts instead.
    let mut changed = params.clone();
    changed.participant_id = "other-guest".into();
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&delegate, &changed, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "request_conflict")
    );
    let decision = DecideDelegatedAction {
        confirmation_id: id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    // The delegate itself cannot approve its own request.
    let mut impostor = owner.clone();
    impostor.participant_id = delegate.delegate_id().to_owned();
    assert!(
        matches!(store.decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&impostor), &decision).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "permission_denied")
    );
    let ack = store
        .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
        .await?;
    assert_eq!(ack.result["state"], "succeeded");
    let receipt = &ack.result["receipt"];
    assert_eq!(receipt["status"], "succeeded");
    assert_eq!(receipt["action"], "kick_participant");
    assert_eq!(receipt["target_id"], json!(GUEST_ID));
    assert_eq!(receipt["confirmation_id"], json!(id));
    for forbidden in [
        "credential",
        "bearer",
        "token",
        "secret",
        "password",
        "workspace",
        "profile",
    ] {
        assert!(
            !serde_json::to_string(receipt)?.contains(forbidden),
            "AI receipt carries {forbidden}: {receipt}"
        );
    }
    // The kick is committed exactly like the owner's own kick of a human.
    assert_eq!(
        store.participant("general", GUEST_ID).await?.status,
        ParticipantStatus::Kicked
    );
    assert_eq!(live_session_state(&store, GUEST_ID).await?, "ended");
    // The canonical removal event and a system line carry the AI's attribution.
    let attributed: Vec<_> = store
        .snapshot("general", 0, 400)
        .await?
        .events
        .into_iter()
        .filter(|event| event.extra.get("confirmation_id") == Some(&json!(id)))
        .collect();
    assert!(attributed.len() >= 2);
    for event in &attributed {
        assert_eq!(event.actor.participant_id, delegate.delegate_id());
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            json!(owner.participant_id)
        );
    }
    assert!(attributed.iter().any(|event| {
        event.event_type == "participant_kicked"
            && event.participant_id.as_deref() == Some(GUEST_ID)
    }));
    assert!(attributed.iter().any(|event| {
        event.message_kind.as_deref() == Some("system")
            && event
                .content
                .as_ref()
                .is_some_and(|text| text.contains("on behalf of"))
    }));
    // Retrying the executed request reads its receipt even though the target is gone.
    let recorded = store
        .request_delegated_participant_kick::<()>(&delegate, &params, |_, _, _| {
            panic!("retry charged")
        })
        .await?
        .0;
    assert_eq!(recorded, *receipt);
    Ok(())
}

#[tokio::test]
async fn kick_refuses_a_missing_owner_host_ai_or_already_removed_target() -> TestResult {
    let (store, _owner, delegate) = kick_fixture().await?;
    // A missing participant cannot be bound.
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&delegate, &params("missing"), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "target_changed")
    );
    // The owner's own gate still protects the local operator.
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&delegate, &params(LOCAL_OPERATOR_PARTICIPANT_ID), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_removal_denied")
    );
    // An AI is never a human kick target; agents use their own removal action.
    assert_eq!(
        store
            .participant("general", delegate.delegate_id())
            .await?
            .participant_type,
        "agent"
    );
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&delegate, &params(delegate.delegate_id()), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "target_changed")
    );
    // A target removed before the request refuses instead of removing again.
    store
        .execute_participant_removal(
            RoomMutationAuthority::TrustedPrincipal(
                &crate::human_session_authority_tests::local_operator_principal(),
            ),
            "removed-first",
            "participant.kick",
            &json!({"participant_id": GUEST_ID}),
        )
        .await?;
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&delegate, &params(GUEST_ID), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "target_changed")
    );
    Ok(())
}

#[tokio::test]
async fn kick_refuses_a_target_re_admitted_before_approval() -> TestResult {
    let (store, owner, delegate) = kick_fixture().await?;
    let id = bound_kick(&store, &delegate, GUEST_ID).await?;
    // The person behind the id is replaced while the confirmation waits.
    let mut tx = store.pool.begin().await?;
    sqlx::query("UPDATE participants SET participant_json=json_set(participant_json,'$.created_at','2000-01-01T00:00:00Z') WHERE room_id='general' AND participant_id=?")
        .bind(GUEST_ID)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    let ack = store
        .decide_delegated_action(
            RoomMutationAuthority::TrustedPrincipal(&owner),
            &DecideDelegatedAction {
                confirmation_id: id,
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(ack.result["state"], "failed");
    assert_eq!(ack.result["receipt"]["code"], "target_changed");
    assert_eq!(
        store.participant("general", GUEST_ID).await?.status,
        ParticipantStatus::Joined
    );
    assert_eq!(live_session_state(&store, GUEST_ID).await?, "active");
    Ok(())
}

#[tokio::test]
async fn kick_refuses_a_target_who_left_and_rejoined_before_approval() -> TestResult {
    let (store, owner, delegate, guest) = rejoinable_fixture().await?;
    let target = guest.principal().participant_id.clone();
    let id = bound_kick(&store, &delegate, &target).await?;
    // The guest leaves: membership ends but the creation identity is kept.
    store
        .execute_room_session_participant_leave(
            &crate::RoomSessionAuthorization::Human(guest),
            &Uuid::new_v4().to_string(),
            &json!({}),
        )
        .await?;
    assert_eq!(
        store.participant("general", &target).await?.status,
        ParticipantStatus::Left
    );
    // The same browser rejoins through a second reusable invite: the participant row
    // returns to Joined with its creation identity unchanged.
    rejoin(&store).await?;
    assert_eq!(
        store.participant("general", &target).await?.status,
        ParticipantStatus::Joined
    );
    let ack = store
        .decide_delegated_action(
            RoomMutationAuthority::TrustedPrincipal(&owner),
            &DecideDelegatedAction {
                confirmation_id: id,
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(ack.result["state"], "failed");
    assert_eq!(ack.result["receipt"]["code"], "target_changed");
    assert_eq!(
        store.participant("general", &target).await?.status,
        ParticipantStatus::Joined
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM human_room_sessions WHERE room_id='general' AND participant_id=? AND state='active'"
        )
        .bind(&target)
        .fetch_one(&store.pool)
        .await?,
        1
    );
    Ok(())
}

/// A guest admitted through a reusable invite, with the live session kept for leave.
async fn rejoinable_fixture() -> Result<
    (
        SqliteStore,
        AuthenticatedPrincipal,
        DelegateRoomAuthority,
        HumanSessionAuthorization,
    ),
    Box<dyn std::error::Error>,
> {
    let store = SqliteStore::open("sqlite::memory:").await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    insert_reusable_invite(&store, REJOIN_SIGNED, REJOIN_JOIN, "rejoin-guest").await?;
    let now = chrono::Utc::now();
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: REJOIN_JOIN,
        },
        REJOIN_BROWSER,
        &HumanAdmissionInput {
            request_id: Uuid::new_v4().to_string(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Rejoin Guest".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: "Host".to_owned(),
            client_id: "rejoin-browser".to_owned(),
            avatar_image_url: String::new(),
        },
    )?;
    assert!(matches!(
        store.admit_human(&request, now).await?,
        HumanAdmissionDecision::Admitted(_)
    ));
    let fingerprint: [u8; 32] = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT session_fingerprint FROM human_room_sessions WHERE state = 'active'",
    )
    .fetch_one(&store.pool)
    .await?
    .try_into()
    .map_err(|value: Vec<u8>| format!("invalid fingerprint length: {}", value.len()))?;
    let guest = store.authorize_human_session(&fingerprint).await?;
    let owner = crate::human_session_authority_tests::local_operator_principal();
    let delegate = director(&store).await?;
    Ok((store, owner, delegate, guest))
}

async fn rejoin(store: &SqliteStore) -> Result<(), Box<dyn std::error::Error>> {
    insert_reusable_invite(store, REJOIN_SIGNED_TWO, REJOIN_JOIN_TWO, "unused").await?;
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode {
            fingerprint: REJOIN_JOIN_TWO,
        },
        REJOIN_BROWSER,
        &HumanAdmissionInput {
            request_id: Uuid::new_v4().to_string(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Rejoin Guest".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: "Host".to_owned(),
            client_id: "rejoin-browser".to_owned(),
            avatar_image_url: String::new(),
        },
    )?;
    assert!(matches!(
        store.admit_human(&request, chrono::Utc::now()).await?,
        HumanAdmissionDecision::Admitted(_)
    ));
    Ok(())
}

async fn insert_reusable_invite(
    store: &SqliteStore,
    signed: [u8; 32],
    join: [u8; 32],
    base_participant_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let now = chrono::Utc::now();
    sqlx::query(
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', ?, 'Rejoin Guest', 'read_write', 0, 0, ?, 0, ?, ?)",
    )
    .bind(hex::encode(&signed[..8]))
    .bind(signed.as_slice())
    .bind(join.as_slice())
    .bind(base_participant_id)
    .bind((now + chrono::Duration::hours(1)).timestamp_micros())
    .bind(agentsassemble_domain::LOCAL_OPERATOR_USER_ID)
    .bind((now - chrono::Duration::minutes(1)).timestamp_micros())
    .execute(&store.pool)
    .await?;
    Ok(())
}

const REJOIN_SIGNED: [u8; 32] = [0x71; 32];
const REJOIN_JOIN: [u8; 32] = [0x72; 32];
const REJOIN_BROWSER: [u8; 32] = [0x73; 32];
const REJOIN_SIGNED_TWO: [u8; 32] = [0x74; 32];
const REJOIN_JOIN_TWO: [u8; 32] = [0x75; 32];

#[tokio::test]
async fn a_guest_ai_cannot_kick_anyone() -> TestResult {
    let (store, session, now) = crate::attendee::connection_tests::fixture().await?;
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
            &json!({"content":"Guest kick"}),
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
    // The guest owner cannot remove anyone, so their AI is refused rather than queued.
    assert!(
        matches!(store.request_delegated_participant_kick::<()>(&authority, &params(GUEST_ID), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_not_permitted")
    );
    assert_eq!(
        store.delegated_direct_actions(&authority).await?,
        vec!["set_message_pin"]
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delegated_action_requests")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    assert_eq!(
        store.participant("general", GUEST_ID).await?.status,
        ParticipantStatus::Joined
    );
    Ok(())
}
