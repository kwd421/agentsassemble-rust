use super::*;
use agentsassemble_domain::{
    ConfirmationDecision, ConfirmedRoomAction, DecideDelegatedAction,
    LOCAL_OPERATOR_PARTICIPANT_ID, ParticipantStatus, RequestRemoveAgent,
};

fn params(agent_id: &str) -> RequestRemoveAgent {
    RequestRemoveAgent {
        request_id: Uuid::new_v4().to_string(),
        agent_id: agent_id.into(),
    }
}

async fn bound_removal(
    store: &SqliteStore,
    delegate: &DelegateRoomAuthority,
    agent_id: &str,
) -> Result<String, Box<dyn std::error::Error>> {
    let pending = store
        .request_delegated_agent_removal(delegate, &params(agent_id), |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation", "{pending}");
    let id = pending["confirmation_id"]
        .as_str()
        .ok_or("confirmation")?
        .to_owned();
    Ok(id)
}

#[tokio::test]
async fn unconfirmed_removal_changes_nothing_and_only_the_owner_approves() -> TestResult {
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let params = params(AGENT_ID);
    let before = store.snapshot("general", 0, 300).await?;
    let pending = store
        .request_delegated_agent_removal(&delegate, &params, |_, _, _| Ok(()))
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    assert_eq!(
        store.snapshot("general", 0, 300).await?.agent_sessions,
        before.agent_sessions
    );
    assert_eq!(
        store.participant("general", AGENT_ID).await?.status,
        ParticipantStatus::Detached
    );
    // The request binds the target's creation identity without any re-read.
    let (request, _, _) = store.approved_removal_snapshot("general", id).await?;
    let ConfirmedRoomAction::RemoveAgent(bound) = &request.action else {
        return Err("bound removal missing".into());
    };
    assert_eq!(bound.agent_id, AGENT_ID);
    assert_eq!(request.target_name, "Terra");
    // Exact retries return the same receipt without a second debit.
    let retry = store
        .request_delegated_agent_removal::<()>(&delegate, &params, |_, _, _| {
            panic!("retry charged")
        })
        .await?
        .0;
    assert_eq!(pending, retry);
    // Retargeting under the same request identity conflicts instead.
    let mut changed = params.clone();
    changed.agent_id = "codex-other".into();
    assert!(
        matches!(store.request_delegated_agent_removal::<()>(&delegate, &changed, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "request_conflict")
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
    assert_eq!(receipt["action"], "remove_agent");
    assert_eq!(receipt["target_id"], json!(AGENT_ID));
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
    // The removal is committed exactly like the owner's own kick.
    assert_eq!(
        store.participant("general", AGENT_ID).await?.status,
        ParticipantStatus::Kicked
    );
    let removed = store
        .snapshot("general", 0, 400)
        .await?
        .agent_sessions
        .into_iter()
        .find(|agent| agent.session_id == AGENT_ID)
        .ok_or("target missing")?;
    assert!(!removed.enabled);
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_runtime_cleanup WHERE room_id='general' AND session_id=?"
        )
        .bind(AGENT_ID)
        .fetch_one(&store.pool)
        .await?,
        1,
        "the runtime cleanup stays durable for the existing cleanup owner"
    );
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
            && event.participant_id.as_deref() == Some(AGENT_ID)
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
        .request_delegated_agent_removal::<()>(&delegate, &params, |_, _, _| {
            panic!("retry charged")
        })
        .await?
        .0;
    assert_eq!(recorded, *receipt);
    Ok(())
}

#[tokio::test]
async fn removal_refuses_a_missing_owner_or_already_removed_target() -> TestResult {
    let (store, _owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    // A missing agent cannot be bound, and the owner's own gate still protects the owner.
    assert!(
        matches!(store.request_delegated_agent_removal::<()>(&delegate, &params("missing"), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "target_changed")
    );
    assert!(
        matches!(store.request_delegated_agent_removal::<()>(&delegate, &params(LOCAL_OPERATOR_PARTICIPANT_ID), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "owner_removal_denied")
    );
    // A target removed before the request refuses instead of removing again.
    store
        .execute_participant_removal(
            RoomMutationAuthority::TrustedPrincipal(
                &crate::human_session_authority_tests::local_operator_principal(),
            ),
            "removed-first",
            "participant.kick",
            &json!({"participant_id": AGENT_ID}),
        )
        .await?;
    assert!(
        matches!(store.request_delegated_agent_removal::<()>(&delegate, &params(AGENT_ID), |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "target_changed")
    );
    // A target replaced while the request waits refuses at approval, changing nothing.
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let id = bound_removal(&store, &delegate, AGENT_ID).await?;
    let mut tx = store.pool.begin().await?;
    sqlx::query("UPDATE agent_sessions SET session_json=json_set(session_json,'$.created_at','2000-01-01T00:00:00Z') WHERE room_id='general' AND session_id=?")
        .bind(AGENT_ID)
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
        store.participant("general", AGENT_ID).await?.status,
        ParticipantStatus::Detached
    );
    Ok(())
}
