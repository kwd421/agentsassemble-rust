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
    let revision = created.outcome.events[0].extra["room_settings"]["settings_revision"]
        .as_str()
        .ok_or("revision")?
        .to_owned();
    let request = DeleteTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: created.outcome.result["target_id"]
            .as_str()
            .ok_or("channel")?
            .into(),
        expected_settings_revision: None,
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
        matches!(store.execute_delegated_atomic(&authority,&DirectRoomAction::RenameTextChannel(agentsassemble_domain::RenameTextChannel {request_id:request.request_id.clone(),channel_id:request.channel_id.clone(),name:"Smuggled".into(),expected_settings_revision:revision}),|_,_,_|Ok(())).await,Err(PersistenceError::CommandRejected{code,..}) if code=="request_conflict")
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
    }
    let request = DeleteTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: created_ids[0].clone(),
        expected_settings_revision: None,
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
    let channels = store.snapshot("general", 0, 100).await?.settings.channels;
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
        let revision = created.outcome.events[0].extra["room_settings"]["settings_revision"]
            .as_str()
            .unwrap()
            .to_owned();
        let request = DeleteTextChannel {
            request_id: Uuid::new_v4().to_string(),
            channel_id: created.outcome.result["target_id"].as_str().unwrap().into(),
            expected_settings_revision: None,
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
                                expected_settings_revision: revision.clone(),
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

async fn create_channel(
    store: &SqliteStore,
    authority: &DelegateRoomAuthority,
    name: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let created = store
        .execute_delegated_atomic(
            authority,
            &DirectRoomAction::CreateTextChannel(CreateTextChannel {
                request_id: Uuid::new_v4().to_string(),
                name: name.into(),
            }),
            |_, _, _| Ok(()),
        )
        .await?
        .0;
    Ok(created.outcome.result["target_id"]
        .as_str()
        .ok_or("channel")?
        .to_owned())
}

async fn request_delete(
    store: &SqliteStore,
    authority: &DelegateRoomAuthority,
    channel_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let pending = store
        .request_delegated_channel_delete::<()>(
            authority,
            &DeleteTextChannel {
                request_id: Uuid::new_v4().to_string(),
                channel_id: channel_id.into(),
                expected_settings_revision: None,
            },
            |_, _, _| Ok(()),
        )
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    Ok(pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned())
}

async fn approve(
    store: &SqliteStore,
    confirmation_id: &str,
) -> Result<crate::CommandOutcome, Box<dyn std::error::Error>> {
    let local = crate::human_session_authority_tests::local_operator_principal();
    Ok(store
        .decide_delegated_action(
            crate::RoomMutationAuthority::TrustedPrincipal(&local),
            &DecideDelegatedAction {
                confirmation_id: confirmation_id.into(),
                decision: ConfirmationDecision::Approve,
                full_access_acknowledged: None,
            },
        )
        .await?)
}

async fn rename_channel(
    store: &SqliteStore,
    authority: &DelegateRoomAuthority,
    channel_id: &str,
    name: &str,
) -> TestResult {
    let revision =
        agentsassemble_domain::public_settings(&store.snapshot("general", 0, 100).await?.settings)?
            .settings_revision;
    store
        .execute_delegated_atomic(
            authority,
            &DirectRoomAction::RenameTextChannel(agentsassemble_domain::RenameTextChannel {
                request_id: Uuid::new_v4().to_string(),
                channel_id: channel_id.into(),
                name: name.into(),
                expected_settings_revision: revision,
            }),
            |_, _, _| Ok(()),
        )
        .await?;
    Ok(())
}

// Two deletions requested in one turn share nothing but the room: approving
// the first must not invalidate the second.
#[tokio::test]
async fn two_pending_deletes_from_one_turn_approve_in_sequence() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let first = create_channel(&store, &authority, "First").await?;
    let second = create_channel(&store, &authority, "Second").await?;
    let first_confirmation = request_delete(&store, &authority, &first).await?;
    let second_confirmation = request_delete(&store, &authority, &second).await?;
    assert_eq!(
        approve(&store, &first_confirmation).await?.result["state"],
        "succeeded"
    );
    assert_eq!(
        store
            .snapshot("general", 0, 100)
            .await?
            .settings
            .channels
            .iter()
            .map(|channel| channel.id.clone())
            .collect::<Vec<_>>(),
        [second.clone()]
    );
    assert_eq!(
        approve(&store, &second_confirmation).await?.result["state"],
        "succeeded"
    );
    assert!(
        store
            .snapshot("general", 0, 100)
            .await?
            .settings
            .channels
            .is_empty()
    );
    Ok(())
}

// The owner must never approve a different target than the card showed.
#[tokio::test]
async fn renamed_delete_target_refuses_target_changed() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let target = create_channel(&store, &authority, "Target").await?;
    let confirmation = request_delete(&store, &authority, &target).await?;
    rename_channel(&store, &authority, &target, "Changed").await?;
    let outcome = approve(&store, &confirmation).await?;
    assert_eq!(outcome.result["state"], "failed");
    assert_eq!(outcome.result["receipt"]["code"], "target_changed");
    let channels = store.snapshot("general", 0, 100).await?.settings.channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].name, "Changed");
    Ok(())
}

// An unrelated channel change before approval leaves this target alone.
#[tokio::test]
async fn unrelated_channel_change_does_not_block_delete_approval() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let target = create_channel(&store, &authority, "Target").await?;
    let other = create_channel(&store, &authority, "Other").await?;
    let confirmation = request_delete(&store, &authority, &target).await?;
    rename_channel(&store, &authority, &other, "Renamed").await?;
    assert_eq!(
        approve(&store, &confirmation).await?.result["state"],
        "succeeded"
    );
    let channels = store.snapshot("general", 0, 100).await?.settings.channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].id, other);
    assert_eq!(channels[0].name, "Renamed");
    assert_eq!(channels[0].position, 0);
    Ok(())
}

// The host emits the request-time settings revision on the wire for old web
// clients. It is informational only: approval binds the target, and rows
// stored without it (revision-less host) still parse and approve.
#[tokio::test]
async fn outgoing_delete_action_carries_settings_revision() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let target = create_channel(&store, &authority, "Target").await?;
    let confirmation = request_delete(&store, &authority, &target).await?;
    let encoded: String = sqlx::query_scalar(
        "SELECT request_json FROM delegated_action_requests WHERE room_id='general' AND confirmation_id=?",
    )
    .bind(&confirmation)
    .fetch_one(&store.pool)
    .await?;
    let request: serde_json::Value = serde_json::from_str(&encoded)?;
    let revision = request["action"]["expected_settings_revision"]
        .as_str()
        .ok_or("expected_settings_revision missing")?;
    assert!(!revision.is_empty());
    // Both wire shapes parse: with the revision (old and new hosts) and
    // without it (the revision-less host).
    let agentsassemble_domain::ConfirmedRoomAction::DeleteTextChannel(with) =
        serde_json::from_value(request["action"].clone())?
    else {
        return Err("action".into());
    };
    assert_eq!(with.expected_settings_revision.as_deref(), Some(revision));
    let mut bare = request["action"].clone();
    bare.as_object_mut()
        .ok_or("action")?
        .remove("expected_settings_revision");
    let agentsassemble_domain::ConfirmedRoomAction::DeleteTextChannel(without): agentsassemble_domain::ConfirmedRoomAction =
        serde_json::from_value(bare)?
    else {
        return Err("action".into());
    };
    assert_eq!(without.expected_settings_revision, None);
    assert_eq!(without.request_id, with.request_id);
    assert_eq!(without.channel_id, with.channel_id);
    Ok(())
}

// Rows stored before per-target binding carry the whole-room revision. It is
// ignored: the stored card name binds the target, so those rows stay
// approvable instead of failing on unrelated room changes.
#[tokio::test]
async fn legacy_row_with_settings_revision_is_honored_per_target() -> TestResult {
    let (store, authority, _, _dir) = fixture().await?;
    let target = create_channel(&store, &authority, "Target").await?;
    let other = create_channel(&store, &authority, "Other").await?;
    let confirmation = request_delete(&store, &authority, &target).await?;
    let encoded: String = sqlx::query_scalar(
        "SELECT request_json FROM delegated_action_requests WHERE room_id='general' AND confirmation_id=?",
    )
    .bind(&confirmation)
    .fetch_one(&store.pool)
    .await?;
    let mut request: serde_json::Value = serde_json::from_str(&encoded)?;
    request["action"]["expected_settings_revision"] =
        serde_json::json!("stale-whole-room-revision");
    sqlx::query("UPDATE delegated_action_requests SET request_json=? WHERE room_id='general' AND confirmation_id=?")
        .bind(serde_json::to_string(&request)?)
        .bind(&confirmation)
        .execute(&store.pool)
        .await?;
    rename_channel(&store, &authority, &other, "Renamed").await?;
    assert_eq!(
        approve(&store, &confirmation).await?.result["state"],
        "succeeded"
    );
    let channels = store.snapshot("general", 0, 100).await?.settings.channels;
    assert_eq!(channels.len(), 1);
    assert_eq!(channels[0].id, other);
    Ok(())
}
