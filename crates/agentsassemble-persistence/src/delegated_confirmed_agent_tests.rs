use super::*;
use agentsassemble_domain::{ConfirmationDecision, DecideDelegatedAction};

#[tokio::test]
async fn approval_reserves_once_binds_configuration_and_rechecks_before_effect() -> TestResult {
    for change_target in [false, true] {
        let (store, owner, _dir) = fixture().await;
        let delegate = director(&store).await?;
        let mut target = crate::agent_lifecycle::load_session(
            &mut store.pool.begin().await?,
            "general",
            AGENT_ID,
        )
        .await?;
        // This lifecycle fixture represents an already human-configured write-capable target.
        target.public.permission_mode = "workspace_write".into();
        target.runtime_profile_key.push_str("-write");
        let mut tx = store.pool.begin().await?;
        crate::agent_lifecycle::save_session(&mut tx, &target).await?;
        tx.commit().await?;
        let request = StartAgent {
            request_id: Uuid::new_v4().to_string(),
            agent_id: AGENT_ID.into(),
        };
        let before = store.snapshot("general", 0, 100).await?.agent_sessions;
        let pending = store
            .request_delegated_agent_start(&delegate, &request, |_, _, _| Ok(()))
            .await?
            .ok_or("confirmation missing")?
            .0;
        assert_eq!(pending["status"], "pending_confirmation");
        assert_eq!(
            store.snapshot("general", 0, 100).await?.agent_sessions,
            before
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lifecycle_command_reservations")
                .fetch_one(&store.pool)
                .await?,
            0
        );
        assert_eq!(
            store
                .request_delegated_agent_start::<()>(&delegate, &request, |_, _, _| panic!(
                    "retry charged"
                ))
                .await?
                .unwrap()
                .0,
            pending
        );
        let id = pending["confirmation_id"].as_str().unwrap();
        let decision = DecideDelegatedAction {
            confirmation_id: id.into(),
            decision: ConfirmationDecision::Approve,
            full_access_acknowledged: None,
        };
        if change_target {
            target.public.model = "another configured model".into();
            let mut tx = store.pool.begin().await?;
            crate::agent_lifecycle::save_session(&mut tx, &target).await?;
            tx.commit().await?;
            let result = store
                .decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decision)
                .await?;
            assert_eq!(result.result["receipt"]["code"], "target_changed");
            assert_eq!(
                sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM lifecycle_command_reservations")
                    .fetch_one(&store.pool)
                    .await?,
                0
            );
            continue;
        }
        let (first, second) = tokio::join!(
            store.decide_delegated_action(
                RoomMutationAuthority::TrustedPrincipal(&owner),
                &decision
            ),
            store.decide_delegated_action(
                RoomMutationAuthority::TrustedPrincipal(&owner),
                &decision
            )
        );
        let (first, second) = (first?, second?);
        assert_eq!(first.result, second.result);
        assert_ne!(first.deduplicated, second.deduplicated);
        assert_eq!(first.result["state"], "executing");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE status='pending'"
            )
            .fetch_one(&store.pool)
            .await?,
            1
        );
        let control = store.confirmed_agent_start("general", id).await?;
        assert!(!control.principal().capabilities.agent_control);
        // Losing the exact delegate role after reservation still forbids external effects.
        store
            .execute_participant_role_update(
                RoomMutationAuthority::TrustedPrincipal(&owner),
                &Uuid::new_v4().to_string(),
                &json!({"participant_id":delegate.delegate_id(),"role":"agent"}),
            )
            .await?;
        let result = store
            .authorize_agent_start_effect(
                AgentControlAuthority::Delegated(&control),
                control.request_id(),
                control.payload(),
                &control.operation_id(),
                "agent.start",
                "handle",
                "runtime-owner",
                "lease",
            )
            .await;
        assert!(
            matches!(result,Err(PersistenceError::CommandRejected{code,..}) if code=="role_required")
        );
        store
            .fail_agent_start_before_effect(
                control.principal(),
                control.request_id(),
                control.payload(),
                &control.operation_id(),
                "role_required",
                "The delegate role ended.",
                "agent.start",
            )
            .await?;
        assert_eq!(
            store
                .request_delegated_agent_start::<()>(&delegate, &request, |_, _, _| panic!(
                    "terminal retry charged"
                ))
                .await?
                .unwrap()
                .0["code"],
            "role_required"
        );
    }
    Ok(())
}

#[tokio::test]
async fn full_access_approval_requires_owner_acknowledgement() -> TestResult {
    let (store, owner, _dir) = fixture().await;
    let delegate = director(&store).await?;
    let mut target =
        crate::agent_lifecycle::load_session(&mut store.pool.begin().await?, "general", AGENT_ID)
            .await?;
    // This lifecycle fixture represents an already human-configured full-access target.
    target.public.permission_mode = "full_access".into();
    target.runtime_profile_key.push_str("-full");
    let mut tx = store.pool.begin().await?;
    crate::agent_lifecycle::save_session(&mut tx, &target).await?;
    tx.commit().await?;
    let request = StartAgent {
        request_id: Uuid::new_v4().to_string(),
        agent_id: AGENT_ID.into(),
    };
    let pending = store
        .request_delegated_agent_start(&delegate, &request, |_, _, _| Ok(()))
        .await?
        .ok_or("confirmation missing")?
        .0;
    assert_eq!(pending["status"], "pending_confirmation");
    let id = pending["confirmation_id"].as_str().unwrap();
    let decide = |acknowledged: Option<bool>| DecideDelegatedAction {
        confirmation_id: id.into(),
        decision: ConfirmationDecision::Approve,
        full_access_acknowledged: acknowledged,
    };
    for without in [None, Some(false)] {
        assert!(
            matches!(store.decide_delegated_action(RoomMutationAuthority::TrustedPrincipal(&owner), &decide(without)).await, Err(PersistenceError::CommandRejected{code,..}) if code=="full_access_acknowledgement_required")
        );
    }
    // The refused request stays pending for a corrected approval.
    let receipt = store
        .decide_delegated_action(
            RoomMutationAuthority::TrustedPrincipal(&owner),
            &decide(Some(true)),
        )
        .await?;
    assert_eq!(receipt.result["state"], "executing");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM lifecycle_command_reservations WHERE status='pending'"
        )
        .fetch_one(&store.pool)
        .await?,
        1
    );
    Ok(())
}
