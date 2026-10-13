use super::*;
use agentsassemble_domain::{
    AgentSessionDraft, ConfirmationDecision, ConfirmationState, ConfirmedRoomAction,
    DecideDelegatedAction, RequestReconfigureAgent,
};

fn params(agent_id: &str, permission_mode: &str) -> RequestReconfigureAgent {
    RequestReconfigureAgent {
        request_id: Uuid::new_v4().to_string(),
        agent_id: agent_id.into(),
        catalog_revision: "catalog-2".into(),
        model: None,
        reasoning_effort: None,
        permission_mode: Some(permission_mode.into()),
        workspace: None,
    }
}

fn draft(agent_id: &str, permission_mode: &str) -> AgentSessionDraft {
    AgentSessionDraft {
        agent_id: agent_id.into(),
        display_name: "Terra".into(),
        provider_kind: "codex_live_session".into(),
        runtime_kind: "live_cli".into(),
        connection_kind: "native_cli_bridge".into(),
        executable: String::new(),
        executable_identity: String::new(),
        workspace: String::new(),
        workspace_identity: String::new(),
        provider_endpoint: String::new(),
        model: "gpt-5.6-terra".into(),
        reasoning_effort: "medium".into(),
        service_tier: "default".into(),
        variant: String::new(),
        execution_harness: "builtin".into(),
        permission_mode: permission_mode.into(),
        max_output_tokens: 0,
        catalog_revision: "catalog-2".into(),
        persona_card_id: String::new(),
        runtime_profile_key: "profile-2".into(),
        transport: "stdio_jsonl".into(),
    }
}

#[tokio::test]
async fn unconfirmed_reconfigure_changes_nothing_and_only_owner_approves() -> TestResult {
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let params = params(AGENT_ID, "workspace_write");
    let draft = draft(AGENT_ID, "workspace_write");
    let before = store.snapshot("general", 0, 300).await?.agent_sessions;
    let pending = store
        .request_delegated_agent_reconfigure(&delegate, &params, "profile-1", &draft, |_, _, _| {
            Ok(())
        })
        .await?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    assert_eq!(
        store.snapshot("general", 0, 300).await?.agent_sessions,
        before
    );
    // Exact retries return the same receipt; the revision may be re-read, so the
    // retry passes the moved revision while the static change fields still bind.
    let retry = store
        .request_delegated_agent_reconfigure::<()>(
            &delegate,
            &params,
            "profile-moved",
            &draft,
            |_, _, _| panic!("retry charged"),
        )
        .await?
        .0;
    assert_eq!(pending, retry);
    // Changed change-fields under the same request identity conflict instead.
    let mut changed = params.clone();
    changed.permission_mode = Some("full_access".into());
    assert!(
        matches!(store.request_delegated_agent_reconfigure::<()>(&delegate, &changed, "profile-1", &draft, |_, _, _| Ok(())).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "request_conflict")
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
    assert_eq!(ack.result["state"], "executing");
    assert_eq!(
        store.snapshot("general", 0, 300).await?.agent_sessions,
        before
    );
    let (request, owner_reference, _) = store.approved_reconfigure_snapshot("general", id).await?;
    let bound = match &request.action {
        ConfirmedRoomAction::ReconfigureAgent(reconfigure) => reconfigure,
        _ => return Err("bound reconfiguration missing".into()),
    };
    assert_eq!(bound.params, params);
    assert_eq!(bound.expected_configuration_revision, "profile-1");
    assert_eq!(bound.draft, draft);
    let payload = crate::reconfigure_execution_payload(&bound.params, &request);
    let drive = store
        .drive_approved_agent_reconfigure("general", id, &bound.draft, &payload, &owner_reference)
        .await?;
    assert_eq!(drive.state, ConfirmationState::Succeeded);
    assert_eq!(drive.receipt["status"], "succeeded");
    assert_eq!(drive.receipt["action"], "reconfigure_agent");
    assert_eq!(drive.receipt["target_id"], json!(AGENT_ID));
    assert_eq!(drive.receipt["confirmation_id"], json!(id));
    let mut receipt_keys: Vec<&str> = drive
        .receipt
        .as_object()
        .ok_or("receipt")?
        .keys()
        .map(String::as_str)
        .collect();
    receipt_keys.sort_unstable();
    assert_eq!(
        receipt_keys,
        vec![
            "action",
            "confirmation_id",
            "event_id",
            "request_id",
            "status",
            "target_id"
        ]
    );
    let configured = store
        .snapshot("general", 0, 300)
        .await?
        .agent_sessions
        .into_iter()
        .find(|agent| agent.session_id == AGENT_ID)
        .ok_or("target missing")?;
    assert_eq!(configured.permission_mode, "workspace_write");
    assert_eq!(
        configured.runtime_status,
        agentsassemble_domain::AgentRuntimeStatus::Stopped
    );
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
        event.message_kind.as_deref() == Some("system")
            && event
                .content
                .as_ref()
                .is_some_and(|text| text.contains("on behalf of"))
    }));
    // Re-driving the recorded execution returns the same receipt without new effects.
    let again = store
        .drive_approved_agent_reconfigure("general", id, &bound.draft, &payload, &owner_reference)
        .await?;
    assert_eq!(again.receipt, drive.receipt);
    assert!(again.new_events.is_empty());
    Ok(())
}

#[tokio::test]
async fn reconfigure_requires_a_stopped_unchanged_target() -> TestResult {
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let params = params(AGENT_ID, "workspace_write");
    let draft = draft(AGENT_ID, "workspace_write");
    let pending = store
        .request_delegated_agent_reconfigure(&delegate, &params, "profile-1", &draft, |_, _, _| {
            Ok(())
        })
        .await?
        .0;
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    let decision = DecideDelegatedAction {
        confirmation_id: id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: None,
    };
    store
        .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
        .await?;
    // The target moved after the request: approval may not reconfigure a stale session.
    let mut target =
        crate::agent_lifecycle::load_session(&mut store.pool.begin().await?, "general", AGENT_ID)
            .await?;
    target.runtime_profile_key = "profile-moved".into();
    let mut tx = store.pool.begin().await?;
    crate::agent_lifecycle::save_session(&mut tx, &target).await?;
    tx.commit().await?;
    let (request, owner_reference, _) = store.approved_reconfigure_snapshot("general", id).await?;
    let ConfirmedRoomAction::ReconfigureAgent(bound) = &request.action else {
        return Err("bound reconfiguration missing".into());
    };
    let payload = crate::reconfigure_execution_payload(&bound.params, &request);
    let drive = store
        .drive_approved_agent_reconfigure("general", id, &bound.draft, &payload, &owner_reference)
        .await?;
    assert_eq!(drive.state, ConfirmationState::Failed);
    assert_eq!(drive.receipt["code"], "target_changed");
    let unchanged = store
        .snapshot("general", 0, 300)
        .await?
        .agent_sessions
        .into_iter()
        .find(|agent| agent.session_id == AGENT_ID)
        .ok_or("target missing")?;
    assert_eq!(unchanged.permission_mode, "meeting_read_only");
    Ok(())
}

#[tokio::test]
async fn full_access_reconfigure_requires_owner_acknowledgement() -> TestResult {
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let params = params(AGENT_ID, "full_access");
    let draft = draft(AGENT_ID, "full_access");
    let pending = store
        .request_delegated_agent_reconfigure(&delegate, &params, "profile-1", &draft, |_, _, _| {
            Ok(())
        })
        .await?
        .0;
    let id = pending["confirmation_id"].as_str().ok_or("confirmation")?;
    let decide = |acknowledged: Option<bool>| DecideDelegatedAction {
        confirmation_id: id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: acknowledged,
    };
    for without in [None, Some(false)] {
        assert!(
            matches!(store.decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decide(without)).await, Err(PersistenceError::CommandRejected { code, .. }) if code == "full_access_acknowledgement_required")
        );
    }
    let receipt = store
        .decide_delegated_action(
            RoomMutationAuthority::TrustedPrincipal(&owner),
            &decide(Some(true)),
        )
        .await?;
    assert_eq!(receipt.result["state"], "executing");
    Ok(())
}
