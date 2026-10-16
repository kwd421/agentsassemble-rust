use super::*;
use agentsassemble_domain::{
    ConfirmationDecision, CreateTextChannel, DecideDelegatedAction, DeleteTextChannel,
    DirectRoomAction,
};

#[tokio::test]
async fn confirmation_is_immutable_owner_private_and_executes_once() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let local = crate::human_session_authority_tests::local_operator_principal();
    let created = store
        .execute_delegated_atomic(
            &authority,
            &DirectRoomAction::CreateTextChannel(CreateTextChannel {
                request_id: Uuid::new_v4().to_string(),
                name: "Delete me".into(),
            }),
            |_, _, _| Ok(()),
        )
        .await?
        .0;
    let request = DeleteTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: created.outcome.result["target_id"]
            .as_str()
            .ok_or("channel")?
            .into(),
        expected_settings_revision:
            created.outcome.events[0].extra["room_settings"]["settings_revision"]
                .as_str()
                .ok_or("revision")?
                .into(),
    };
    let message = store
        .execute_channel_message(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &Uuid::new_v4().to_string(),
            &json!({"channel_id":request.channel_id,"content":"Retired channel message"}),
        )
        .await?;
    let before = store.snapshot("general", 0, 100).await?.settings;
    let (pending, _) = store
        .request_delegated_channel_delete::<()>(&authority, &request, |_, _, _| Ok(()))
        .await?;
    assert_eq!(store.snapshot("general", 0, 100).await?.settings, before);
    assert_eq!(
        store
            .request_delegated_channel_delete::<()>(&authority, &request, |_, _, _| panic!(
                "retry charged"
            ))
            .await?
            .0,
        pending
    );
    assert!(
        matches!(store.execute_delegated_atomic(&authority,&DirectRoomAction::RenameTextChannel(agentsassemble_domain::RenameTextChannel {request_id:request.request_id.clone(),channel_id:request.channel_id.clone(),name:"Smuggled".into(),expected_settings_revision:request.expected_settings_revision.clone()}),|_,_,_|Ok(())).await,Err(PersistenceError::CommandRejected{code,..}) if code=="request_conflict")
    );
    let mut changed = request.clone();
    changed.channel_id = "different".into();
    assert!(
        matches!(store.request_delegated_channel_delete::<()>(&authority,&changed,|_,_,_|Ok(())).await,Err(PersistenceError::CommandRejected{code,..}) if code=="request_conflict")
    );
    let payload = _dir.path().join("payload.sqlite");
    let mut backup = crate::backup_database::create_payload(&payload).await?;
    let mut live = store.pool.acquire().await?;
    crate::backup_database::reconstruct(
        &mut live,
        &mut backup,
        false,
        crate::CURRENT_SCHEMA_VERSION,
    )
    .await?;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM delegated_action_requests")
            .fetch_one(&mut backup)
            .await?,
        0
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT COUNT(*) FROM room_events WHERE json_extract(event_json,'$.delegated_action_request') IS NOT NULL").fetch_one(&mut backup).await?,0);
    drop(live);
    sqlx::Connection::close(backup).await?;
    let decision = DecideDelegatedAction {
        confirmation_id: pending["confirmation_id"]
            .as_str()
            .ok_or("confirmation")?
            .into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    crate::human_admission_store::tests::insert_invite(
        &store,
        [51; 32],
        [52; 32],
        "guest",
        1,
        Utc::now(),
    )
    .await;
    let guest_request = crate::human_admission_store::tests::prepared(
        [52; 32],
        [53; 32],
        &Uuid::new_v4().to_string(),
        "Guest",
    );
    crate::human_admission_store::tests::admitted(
        store.admit_human(&guest_request, Utc::now()).await?,
    );
    let guest = store
        .authorize_human_session(
            &crate::human_session_authority_tests::session_fingerprint(&store).await,
        )
        .await?;
    assert!(
        matches!(store.decide_delegated_action(crate::RoomMutationAuthority::HumanSession(&guest), &decision).await, Err(PersistenceError::CommandRejected{code,..}) if code=="permission_denied")
    );
    let mut another_operator = guest.principal().clone();
    another_operator.is_operator = true;
    another_operator.capabilities = local.capabilities.clone();
    assert!(
        matches!(store.decide_delegated_action(crate::RoomMutationAuthority::TrustedPrincipal(&another_operator), &decision).await, Err(PersistenceError::CommandRejected{code,..}) if code=="permission_denied")
    );
    let owner = store
        .snapshot_for(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            0,
            100,
        )
        .await?;
    assert_eq!(owner.delegated_action_requests.len(), 1);
    let ai = authority.resolve(&mut store.pool.begin().await?).await?;
    let other = store
        .snapshot_for(crate::RoomMutationAuthority::TrustedPrincipal(&ai), 0, 100)
        .await?;
    assert!(other.delegated_action_requests.is_empty());
    for event in &owner.events {
        if event.event_type == "delegated_action_requested" {
            assert_eq!(
                agentsassemble_domain::public_event_for_principal(event, &ai).event_type,
                "event_hidden"
            );
            let mut operator = local.clone();
            operator.participant_id = "other".into();
            assert_eq!(
                agentsassemble_domain::public_event_for_principal(event, &operator).event_type,
                "event_hidden"
            );
            let archived = crate::backup_json::event(serde_json::to_value(event)?)?;
            assert_eq!(archived.event_type, "event_hidden");
            assert!(archived.extra.get("delegated_action_request").is_none());
        }
    }
    assert!(
        store
            .decide_delegated_action(
                crate::RoomMutationAuthority::TrustedPrincipal(&ai),
                &decision
            )
            .await
            .is_err()
    );
    let (a, b) = tokio::join!(
        store.decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &decision
        ),
        store.decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &decision
        )
    );
    let (a, b) = (a?, b?);
    assert_eq!(a.result, b.result);
    assert_ne!(a.deduplicated, b.deduplicated);
    assert_eq!(a.result["state"], "succeeded");
    assert!(
        store
            .snapshot("general", 0, 100)
            .await?
            .settings
            .channels
            .is_empty()
    );
    assert_eq!(
        store
            .request_delegated_channel_delete::<()>(&authority, &request, |_, _, _| panic!(
                "retry charged"
            ))
            .await?
            .0,
        a.result["receipt"]
    );
    let deleted = store
        .snapshot("general", 0, 100)
        .await?
        .events
        .into_iter()
        .find(|event| event.id == message.event.id)
        .unwrap();
    assert_eq!(deleted.content.as_deref(), Some(""));
    assert_eq!(deleted.extra["message_deleted"], true);
    let mut declined = decision;
    declined.decision = ConfirmationDecision::Decline;
    assert!(
        store
            .decide_delegated_action(
                crate::RoomMutationAuthority::TrustedPrincipal(&local),
                &declined
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn approved_delete_of_a_non_last_channel_keeps_dense_positions() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let local = crate::human_session_authority_tests::local_operator_principal();
    let mut created_ids = Vec::new();
    let mut revision = String::new();
    for name in ["First", "Second"] {
        let created = store
            .execute_delegated_atomic(
                &authority,
                &DirectRoomAction::CreateTextChannel(CreateTextChannel {
                    request_id: Uuid::new_v4().to_string(),
                    name: name.into(),
                }),
                |_, _, _| Ok(()),
            )
            .await?
            .0;
        created_ids.push(
            created.outcome.result["target_id"]
                .as_str()
                .ok_or("channel")?
                .to_owned(),
        );
        revision = created.outcome.events[0].extra["room_settings"]["settings_revision"]
            .as_str()
            .ok_or("revision")?
            .to_owned();
    }
    let request = DeleteTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: created_ids[0].clone(),
        expected_settings_revision: revision,
    };
    let pending = store
        .request_delegated_channel_delete::<()>(&authority, &request, |_, _, _| Ok(()))
        .await?
        .0;
    let outcome = store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &DecideDelegatedAction {
                confirmation_id: pending["confirmation_id"]
                    .as_str()
                    .ok_or("confirmation")?
                    .into(),
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?;
    assert_eq!(outcome.result["state"], "succeeded");
    let channels = store
        .snapshot("general", 0, 100)
        .await?
        .settings
        .channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].id, created_ids[1]);
    assert_eq!(channels[0].position, 0);
    Ok(())
}

#[tokio::test]
async fn declines_expiry_changed_target_and_revoked_authority_are_terminal() -> TestResult {
    for case in ["decline", "expiry", "target", "owner", "delegate"] {
        let (store, authority, _, _dir) = fixture().await?;
        let local = crate::human_session_authority_tests::local_operator_principal();
        let created = store
            .execute_delegated_atomic(
                &authority,
                &DirectRoomAction::CreateTextChannel(CreateTextChannel {
                    request_id: Uuid::new_v4().to_string(),
                    name: "Target".into(),
                }),
                |_, _, _| Ok(()),
            )
            .await?
            .0;
        let request = DeleteTextChannel {
            request_id: Uuid::new_v4().to_string(),
            channel_id: created.outcome.result["target_id"].as_str().unwrap().into(),
            expected_settings_revision:
                created.outcome.events[0].extra["room_settings"]["settings_revision"]
                    .as_str()
                    .unwrap()
                    .into(),
        };
        let pending = store
            .request_delegated_channel_delete(&authority, &request, |_, _, _| Ok(()))
            .await?
            .0;
        let id = pending["confirmation_id"].as_str().unwrap();
        match case {
            "expiry" => {
                sqlx::query(
                    "UPDATE delegated_action_requests SET expires_at=1 WHERE confirmation_id=?",
                )
                .bind(id)
                .execute(&store.pool)
                .await?;
            }
            "target" => {
                store
                    .execute_delegated_atomic(
                        &authority,
                        &DirectRoomAction::RenameTextChannel(
                            agentsassemble_domain::RenameTextChannel {
                                request_id: Uuid::new_v4().to_string(),
                                channel_id: request.channel_id.clone(),
                                name: "Changed".into(),
                                expected_settings_revision: request
                                    .expected_settings_revision
                                    .clone(),
                            },
                        ),
                        |_, _, _| Ok(()),
                    )
                    .await?;
            }
            "owner" => {
                store
                    .execute_participant_mute(
                        crate::RoomMutationAuthority::TrustedPrincipal(&local),
                        &Uuid::new_v4().to_string(),
                        &json!({"participant_id":local.participant_id,"muted":true}),
                    )
                    .await?;
            }
            "delegate" => {
                store
                    .execute_participant_role_update(
                        crate::RoomMutationAuthority::TrustedPrincipal(&local),
                        &Uuid::new_v4().to_string(),
                        &json!({"participant_id":authority.delegate_id(),"role":"agent"}),
                    )
                    .await?;
            }
            _ => {}
        }
        let decision = DecideDelegatedAction {
            confirmation_id: id.into(),
            decision: if case == "decline" {
                ConfirmationDecision::Decline
            } else {
                ConfirmationDecision::Approve
            },
            full_access_acknowledged: None,
        };
        let receipt = store
            .decide_delegated_action(
                crate::RoomMutationAuthority::TrustedPrincipal(&local),
                &decision,
            )
            .await?;
        let expected = match case {
            "decline" => "confirmation_declined",
            "expiry" => "confirmation_expired",
            "target" => "target_changed",
            _ => "confirmation_cancelled",
        };
        assert_eq!(receipt.result["receipt"]["code"], expected);
        assert_eq!(
            store
                .snapshot("general", 0, 100)
                .await?
                .settings
                .channels
                .len(),
            1
        );
        assert_eq!(
            store
                .request_delegated_channel_delete::<()>(&authority, &request, |_, _, _| panic!(
                    "terminal retry charged"
                ))
                .await?
                .0["code"],
            expected
        );
        let ai = match &authority {
            DelegateRoomAuthority::Connector(session) => session.principal(),
            _ => unreachable!(),
        };
        let mut tx = store.pool.begin().await?;
        let outcomes = crate::delegated_confirmation_lifecycle::outcomes_in(&mut tx, ai).await?;
        assert_eq!(outcomes[0].receipt["code"], expected);
        assert_eq!(outcomes[0].request_id, request.request_id);
    }
    Ok(())
}
