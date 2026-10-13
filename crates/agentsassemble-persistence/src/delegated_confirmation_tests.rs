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
    let mut changed = request.clone();
    changed.channel_id = "different".into();
    assert!(
        matches!(store.request_delegated_channel_delete::<()>(&authority,&changed,|_,_,_|Ok(())).await,Err(PersistenceError::CommandRejected{code,..}) if code=="request_conflict")
    );
    let decision = DecideDelegatedAction {
        confirmation_id: pending["confirmation_id"]
            .as_str()
            .ok_or("confirmation")?
            .into(),
        decision: ConfirmationDecision::Approve,
    };
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
            assert!(archived.extra.is_empty());
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
