use super::*;
use agentsassemble_domain::{ParticipantRole, ParticipantStatus};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn fixture() -> Result<
    (
        SqliteStore,
        DelegateRoomAuthority,
        SetMessagePin,
        tempfile::TempDir,
    ),
    Box<dyn std::error::Error>,
> {
    let directory = tempfile::tempdir()?;
    let store = SqliteStore::open_path(&directory.path().join("live.sqlite")).await?;
    store
        .bootstrap_local_authority(&Uuid::new_v4().to_string(), "Host")
        .await?;
    store
        .create_room_for_local_operator(&Uuid::new_v4().to_string(), "general", "General")
        .await?;
    let now = Utc::now();
    let manager = crate::room_user_identity::test_authority(&store).await;
    let invite = store
        .create_connector_invite(
            &crate::RoomManagerAuthority::Local(manager),
            Uuid::new_v4(),
            InviteScope::ReadWrite,
            now,
        )
        .await?;
    let admitted = store
        .admit_connector(
            &Sha256::digest(invite.invite_bearer.as_bytes()).into(),
            &[44; 32],
            Uuid::new_v4(),
            "Director",
            now,
        )
        .await?;
    let local = crate::human_session_authority_tests::local_operator_principal();
    store.execute_participant_role_update(crate::RoomMutationAuthority::TrustedPrincipal(&local), "director-role", &json!({"participant_id":admitted.authorization.principal().participant_id,"role":"director"})).await?;
    let message = store
        .execute_message(
            &local,
            "pin-target",
            "message.send",
            &json!({"content":"Pin target"}),
        )
        .await?;
    Ok((
        store,
        DelegateRoomAuthority::Connector(admitted.authorization),
        SetMessagePin {
            channel_id: "lobby".into(),
            message_id: message.event.id,
            pinned: true,
            request_id: Uuid::new_v4().to_string(),
        },
        directory,
    ))
}

#[tokio::test]
async fn pin_receipt_attribution_budget_and_atomic_replay_share_human_core() -> TestResult {
    let (store, authority, mut request, _directory) = fixture().await?;
    let charges = AtomicUsize::new(0);
    let charge = |owner: &str, delegate: &str, _bytes| {
        assert_eq!(owner, agentsassemble_domain::LOCAL_OPERATOR_USER_ID);
        assert_ne!(owner, delegate);
        charges.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    let (first, _) = store
        .set_delegated_message_pin(&authority, &request, charge)
        .await?;
    let (retry, charged) = store
        .set_delegated_message_pin(&authority, &request, charge)
        .await?;
    assert!(retry.deduplicated && charged.is_none());
    assert_eq!(first.result, retry.result);
    assert_eq!(charges.load(Ordering::SeqCst), 1);
    assert_eq!(
        first.event.extra["on_behalf_of_participant_id"],
        agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
    );
    assert_eq!(first.event.message_kind.as_deref(), Some("system"));
    assert!(
        first
            .event
            .content
            .as_ref()
            .is_some_and(|text| text.contains("on behalf of Host"))
    );
    let manager = crate::room_user_identity::test_authority(&store).await;
    assert_eq!(store.local_message_pins(&manager, "lobby").await?.len(), 1);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM room_events WHERE json_extract(event_json,'$.request_id')=?",
    )
    .bind(&request.request_id)
    .fetch_one(&store.pool)
    .await?;
    assert_eq!(count, 1);
    request.pinned = false;
    assert!(matches!(
        store
            .set_delegated_message_pin(&authority, &request, charge)
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    request.request_id = Uuid::new_v4().to_string();
    let failure = store
        .set_delegated_message_pin(&authority, &request, |_, _, _| {
            Err::<(), _>(rejected("rate_limited", "No budget."))
        })
        .await;
    assert!(failure.is_err());
    assert_eq!(
        store.local_message_pins(&manager, "lobby").await?.len(),
        1,
        "admission rejection rolls back pin state"
    );
    store
        .set_delegated_message_pin(&authority, &request, charge)
        .await?;
    assert!(
        store
            .local_message_pins(&manager, "lobby")
            .await?
            .is_empty()
    );
    let directory = tempfile::tempdir()?;
    let payload = directory.path().join("export.sqlite");
    store
        .export_backup_payload(
            &crate::ServerOwnerAuthority::LocalOperator,
            &directory.path().join("snapshot.sqlite"),
            &payload,
        )
        .await?;
    crate::backup_database::reconstruct_payload(
        &payload,
        &directory.path().join("restored.sqlite"),
        crate::CURRENT_SCHEMA_VERSION,
    )
    .await?;

    Ok(())
}

#[tokio::test]
async fn next_call_observes_owner_mute_removal_delegate_demotion_and_revocation() -> TestResult {
    let (store, authority, request, _directory) = fixture().await?;
    let DelegateRoomAuthority::Connector(connector) = &authority else {
        unreachable!()
    };
    let mut owner = store
        .participant(
            "general",
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await?;
    let mut delegate = store
        .participant("general", &connector.principal().participant_id)
        .await?;
    for state in 0..5 {
        owner.muted = state == 0;
        owner.status = if state == 1 {
            ParticipantStatus::Left
        } else {
            ParticipantStatus::Joined
        };
        delegate.role = if state == 2 {
            ParticipantRole::Agent
        } else {
            ParticipantRole::Director
        };
        delegate.muted = state == 3;
        let mut tx = store.pool.begin().await?;
        for participant in [&owner, &delegate] {
            crate::participant_rows::save_participant_exact(
                &mut tx,
                "general",
                &participant.participant_id,
                participant,
            )
            .await?;
        }
        if state == 4 {
            sqlx::query("UPDATE room_connector_invites SET revoked=1")
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        assert!(
            store
                .set_delegated_message_pin(&authority, &request, |_, _, _| Ok(()))
                .await
                .is_err()
        );
        if state == 0 {
            let local = crate::room_user_identity::test_authority(&store).await;
            assert!(
                store
                    .set_local_message_pin(&local, "lobby", &request.message_id, true)
                    .await
                    .is_err(),
                "owner and delegate share the mute gate"
            );
        }
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM room_message_pins")
            .fetch_one(&store.pool)
            .await?,
        0
    );
    Ok(())
}

#[tokio::test]
async fn guest_pin_rechecks_scope_and_disconnect_of_the_exact_parent_on_retry() -> TestResult {
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
    let message = store
        .execute_authorized_message_with_turn(
            crate::RoomMutationAuthority::HumanSession(&human),
            &Uuid::new_v4().to_string(),
            "message.send",
            &json!({"content":"Guest pin"}),
        )
        .await?;
    let turn = store
        .deliver_attendee_turn(&connection, now)
        .await?
        .ok_or("turn missing")?;
    let local = crate::human_session_authority_tests::local_operator_principal();
    store
        .execute_participant_role_update(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
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
    assert_direct_rejections(&store, &authority, "owner_not_permitted").await;
    // Agent removal is not available to an owner without room-manage authority, and
    // the refused request stores nothing.
    assert!(
        matches!(store.request_delegated_agent_removal::<()>(&authority, &agentsassemble_domain::RequestRemoveAgent { request_id: Uuid::new_v4().to_string(), agent_id: "missing".into() }, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_not_permitted")
    );
    assert_eq!(
        store.delegated_direct_actions(&authority).await?,
        vec!["set_message_pin"]
    );
    let request = SetMessagePin {
        channel_id: "lobby".into(),
        message_id: message.outcome.event.id,
        pinned: true,
        request_id: Uuid::new_v4().to_string(),
    };
    store
        .set_delegated_message_pin(&authority, &request, |_, _, _| Ok(()))
        .await?;
    // Change only the bound session's current scope: no other session may substitute.
    set_guest_scope(&store, "read_only").await?;
    assert_direct_rejections(&store, &authority, "owner_scope_read_only").await;
    assert!(!store.delegated_pin_available(&authority).await?);
    assert!(
        matches!(store.set_delegated_message_pin(&authority,&request,|_,_,_|Ok(())).await, Err(PersistenceError::CommandRejected {code,..}) if code=="owner_scope_read_only")
    );
    set_guest_scope(&store, "read_write").await?;
    let channel = "p".repeat(43);
    sqlx::query("UPDATE human_room_sessions SET secure_channel_id=?,secure_client_key_fingerprint=? WHERE session_fingerprint=?").bind(&channel).bind([7_u8;32].as_slice()).bind(fingerprint.as_slice()).execute(&store.pool).await?;
    assert!(store.delegated_pin_available(&authority).await?);
    store.disconnect_secure_channel(&channel).await?;
    assert_direct_rejections(&store, &authority, "owner_authority_unavailable").await;
    assert!(!store.delegated_pin_available(&authority).await?);
    assert!(
        matches!(store.set_delegated_message_pin(&authority,&request,|_,_,_|Ok(())).await, Err(PersistenceError::CommandRejected {code,..}) if code=="owner_authority_unavailable")
    );
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM room_events WHERE json_extract(event_json,'$.request_id')=?",
    )
    .bind(&request.request_id)
    .fetch_one(&store.pool)
    .await?;
    assert_eq!(count, 1);
    Ok(())
}

async fn set_guest_scope(store: &SqliteStore, scope: &str) -> TestResult {
    let mut tx = store.pool.begin().await?;
    sqlx::query("PRAGMA defer_foreign_keys=ON")
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE room_invites SET invite_scope=?")
        .bind(scope)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE human_room_sessions SET invite_scope=?")
        .bind(scope)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

#[path = "delegated_direct_tests.rs"]
mod direct;

async fn assert_direct_rejections(
    store: &SqliteStore,
    authority: &DelegateRoomAuthority,
    expected: &str,
) {
    use agentsassemble_domain::*;
    let id = || Uuid::new_v4().to_string();
    for request in [
        DirectRoomAction::CreateTextChannel(CreateTextChannel {
            request_id: id(),
            name: "Denied".into(),
        }),
        DirectRoomAction::RenameTextChannel(RenameTextChannel {
            request_id: id(),
            channel_id: "missing".into(),
            name: "Denied".into(),
            expected_settings_revision: "stale".into(),
        }),
        DirectRoomAction::SetParticipantMute(SetParticipantMute {
            request_id: id(),
            participant_id: "missing".into(),
            muted: true,
        }),
        DirectRoomAction::StartAgent(StartAgent {
            request_id: id(),
            agent_id: "missing".into(),
        }),
        DirectRoomAction::StopAgent(StopAgent {
            request_id: id(),
            agent_id: "missing".into(),
        }),
    ] {
        let error = match request {
            DirectRoomAction::StartAgent(_) | DirectRoomAction::StopAgent(_) => store
                .admit_delegated_agent_control(authority, &request, |_, _, _| Ok(()))
                .await
                .err(),
            _ => store
                .execute_delegated_atomic(authority, &request, |_, _, _| Ok(()))
                .await
                .err(),
        };
        assert!(
            matches!(error,Some(PersistenceError::CommandRejected {code,..}) if code == expected)
        );
    }
}

#[path = "delegated_confirmation_tests.rs"]
mod confirmations;

#[path = "delegated_confirmation_turn_tests.rs"]
mod turn_survival;
