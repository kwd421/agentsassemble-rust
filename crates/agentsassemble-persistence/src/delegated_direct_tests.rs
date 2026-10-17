use super::*;
use agentsassemble_domain::{
    CreateTextChannel, DirectRoomAction, RenameTextChannel, SetParticipantMute,
};

async fn settings(store: &SqliteStore) -> Result<serde_json::Value, Box<dyn std::error::Error>> {
    let raw: String = sqlx::query_scalar("SELECT settings_json FROM rooms WHERE room_id='general'")
        .fetch_one(&store.pool)
        .await?;
    Ok(serde_json::from_str(&raw)?)
}
fn create(name: &str) -> DirectRoomAction {
    DirectRoomAction::CreateTextChannel(CreateTextChannel {
        name: name.into(),
        request_id: Uuid::new_v4().to_string(),
    })
}
fn code(error: PersistenceError) -> String {
    match error {
        PersistenceError::CommandRejected { code, .. } => code.to_string(),
        other => panic!("unexpected failure: {other:?}"),
    }
}
#[tokio::test]
async fn channels_preserve_other_settings_and_bind_replay_revision_and_limits() -> TestResult {
    let (store, authority, _, _directory) = fixture().await?;
    let before = settings(&store).await?;
    let request = create("Planning");
    let charges = AtomicUsize::new(0);
    let charge = |owner: &str, delegate: &str, _| {
        assert_ne!(owner, delegate);
        charges.fetch_add(1, Ordering::SeqCst);
        Ok(())
    };
    let (first, _) = store
        .execute_delegated_atomic(&authority, &request, charge)
        .await?;
    let (replay, debit) = store
        .execute_delegated_atomic(&authority, &request, charge)
        .await?;
    assert_eq!(first.outcome.result, replay.outcome.result);
    assert!(replay.outcome.deduplicated && debit.is_none());
    assert_eq!(charges.load(Ordering::SeqCst), 1);
    assert_eq!(
        first.outcome.event.extra["on_behalf_of_participant_id"],
        agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
    );
    let canonical = &first.outcome.events[0];
    assert_eq!(canonical.event_type, "room_settings_updated");
    for event in &first.outcome.events {
        assert_eq!(event.actor.participant_id, authority.delegate_id());
        assert_eq!(event.actor.participant_type, "agent");
        assert_eq!(event.actor_id.as_deref(), Some(authority.delegate_id()));
        assert_eq!(event.actor_type.as_deref(), Some("agent"));
        assert_eq!(
            event.extra["on_behalf_of_participant_id"],
            agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID
        );
    }
    assert_eq!(
        canonical.extra["actor_participant_id"],
        first.outcome.event.extra["actor_participant_id"]
    );
    let mut after = settings(&store).await?;
    let channel = after["channels"][0]["id"]
        .as_str()
        .ok_or("channel missing")?
        .to_owned();
    after["channels"] = before["channels"].clone();
    assert_eq!(before, after);
    let revision = canonical.extra["room_settings"]["settings_revision"]
        .as_str()
        .ok_or("revision missing")?
        .to_owned();
    let rename = DirectRoomAction::RenameTextChannel(RenameTextChannel {
        request_id: Uuid::new_v4().to_string(),
        channel_id: channel.clone(),
        name: "Renamed".into(),
        expected_settings_revision: revision,
    });
    store
        .execute_delegated_atomic(&authority, &rename, charge)
        .await?;
    assert_eq!(settings(&store).await?["channels"][0]["name"], "Renamed");
    let DirectRoomAction::RenameTextChannel(mut stale) = rename else {
        unreachable!()
    };
    stale.request_id = Uuid::new_v4().to_string();
    assert_eq!(
        code(
            store
                .execute_delegated_atomic(
                    &authority,
                    &DirectRoomAction::RenameTextChannel(stale.clone()),
                    charge
                )
                .await
                .unwrap_err()
        ),
        "settings_conflict"
    );
    stale.channel_id = "c000000000000".into();
    assert_eq!(
        code(
            store
                .execute_delegated_atomic(
                    &authority,
                    &DirectRoomAction::RenameTextChannel(stale),
                    charge
                )
                .await
                .unwrap_err()
        ),
        "channel_not_found"
    );
    let DirectRoomAction::CreateTextChannel(mut changed) = request else {
        unreachable!()
    };
    changed.name = "Different".into();
    assert!(matches!(
        store
            .execute_delegated_atomic(
                &authority,
                &DirectRoomAction::CreateTextChannel(changed),
                charge
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert!(
        store
            .execute_delegated_atomic(&authority, &create(&"x".repeat(61)), charge)
            .await
            .is_err()
    );
    assert!(serde_json::from_value::<DirectRoomAction>(json!({"action":"create_text_channel","request_id":Uuid::new_v4(),"name":"x","topic":"smuggled"})).is_err());
    let snapshot = settings(&store).await?;
    assert!(
        store
            .execute_delegated_atomic(&authority, &create("Rejected"), |_, _, _| Err::<(), _>(
                PersistenceError::CommandRejected {
                    code: "rate_limited".into(),
                    message: "full".into()
                }
            ))
            .await
            .is_err()
    );
    assert_eq!(snapshot, settings(&store).await?);
    Ok(())
}

#[tokio::test]
async fn mute_shares_human_targets_and_next_call_revocation() -> TestResult {
    let (store, authority, _, _directory) = fixture().await?;
    let owner_id = agentsassemble_domain::LOCAL_OPERATOR_PARTICIPANT_ID;
    for muted in [true, false] {
        let request = DirectRoomAction::SetParticipantMute(SetParticipantMute {
            request_id: Uuid::new_v4().to_string(),
            participant_id: owner_id.into(),
            muted,
        });
        let result = store
            .execute_delegated_atomic(&authority, &request, |_, _, _| Ok(()))
            .await;
        if muted {
            assert_eq!(code(result.unwrap_err()), "owner_not_permitted");
        } else {
            result?;
        }
        assert!(!store.participant("general", owner_id).await?.muted);
    }
    let mut target = store.participant("general", owner_id).await?;
    target.participant_id = "mute-target".into();
    sqlx::query(
        "INSERT INTO participants(room_id,participant_id,participant_json) VALUES('general',?,?)",
    )
    .bind(&target.participant_id)
    .bind(serde_json::to_string(&target)?)
    .execute(&store.pool)
    .await?;
    for muted in [true, false] {
        let request = DirectRoomAction::SetParticipantMute(SetParticipantMute {
            request_id: Uuid::new_v4().to_string(),
            participant_id: target.participant_id.clone(),
            muted,
        });
        let (result, _) = store
            .execute_delegated_atomic(&authority, &request, |_, _, _| Ok(()))
            .await?;
        assert_eq!(
            store
                .participant("general", &target.participant_id)
                .await?
                .muted,
            muted
        );
        assert_eq!(result.outcome.events[0].event_type, "participant_muted");
        for event in &result.outcome.events {
            assert_eq!(event.actor.participant_id, authority.delegate_id());
            assert_eq!(event.actor.participant_type, "agent");
            assert_eq!(event.extra["on_behalf_of_participant_id"], owner_id);
        }
        assert_eq!(
            result.outcome.event.extra["target_id"],
            target.participant_id
        );
        // The room-visible attribution line carries the structured mute state
        // so the UI never parses the English sentence (it holds display names).
        assert_eq!(result.outcome.event.extra["muted"], json!(muted));
        assert!(
            store
                .execute_delegated_atomic(&authority, &request, |_, _, _| Ok(()))
                .await?
                .0
                .outcome
                .deduplicated
        );
    }
    let mut owner = store.participant("general", owner_id).await?;
    let mut delegate = store
        .participant("general", authority.delegate_id())
        .await?;
    for (state, expected) in [
        (0, "owner_muted"),
        (1, "owner_authority_unavailable"),
        (2, "role_required"),
        (3, "delegate_authority_expired"),
    ] {
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
        tx.commit().await?;
        assert_direct_rejections(&store, &authority, expected).await;
        assert!(store.delegated_direct_actions(&authority).await?.is_empty());
    }
    Ok(())
}
