use super::*;

#[tokio::test]
async fn completion_recovery_preserves_exact_identity_and_commits_once()
-> Result<(), PersistenceError> {
    for record_running_first in [false, true] {
        let (store, principal, _directory) = fixture().await;
        let mutation = store
            .execute_message_with_turn(
                &principal,
                "recover-completion",
                "message.send",
                &json!({"content": "@Terra finish this observation"}),
            )
            .await?;
        let assignment = &mutation.assignments[0];
        let start = store
            .authorize_provider_turn_start(
                "general",
                AGENT_ID,
                assignment.turn_generation,
                &assignment.turn_id,
            )
            .await?;
        if record_running_first {
            store
                .mark_provider_turn_running(&start, "retained-result")
                .await?;
            store
                .mark_provider_turn_running(&start, "retained-result")
                .await?;
        }
        let recovery = store
            .mark_provider_turn_recovery_required(&start, Some("commit failed"))
            .await
            .unwrap_or_else(|error| panic!("publish recovery: {error}"));
        assert_eq!(recovery.events.len(), 1);
        let repeated = store
            .mark_provider_turn_recovery_required(&start, Some("commit failed"))
            .await
            .unwrap_or_else(|error| panic!("repeat recovery: {error}"));
        assert!(repeated.events.is_empty());
        let before = stored_session(&store).await;
        assert!(before.public.recovery_required);
        assert_eq!(before.public.last_provider_sync_seq, 0);
        assert_eq!(before.public.active_turn_id, assignment.turn_id);

        store
            .mark_provider_turn_running(&start, "retained-result")
            .await
            .unwrap_or_else(|error| panic!("restore exact result identity: {error}"));
        let error = store
            .mark_provider_turn_running(&start, "substituted-result")
            .await
            .err()
            .unwrap_or_else(|| {
                panic!("a different provider result must not replace the retained identity")
            });
        assert_rejection_code(&error, "stale_provider_turn");
        let mut forged = start.clone();
        forged.start_dispatch_nonce = uuid::Uuid::new_v4().to_string();
        let error = store
            .mark_provider_turn_running(&forged, "retained-result")
            .await
            .err()
            .unwrap_or_else(|| panic!("a different dispatch must be rejected"));
        assert_rejection_code(&error, "stale_provider_turn");
        let execution = store
            .provider_turn_execution("general", AGENT_ID, assignment.turn_generation)
            .await
            .unwrap_or_else(|error| panic!("read recovery execution: {error}"));
        assert_eq!(
            execution.phase,
            crate::ProviderTurnExecutionPhase::RecoveryRequired
        );

        let completed = store
            .decline_agent_turn(
                "general",
                AGENT_ID,
                authority(&start, "retained-result", None),
                "nothing_useful_to_add",
            )
            .await
            .unwrap_or_else(|error| panic!("commit retained result: {error}"));
        assert_eq!(
            event_types(&completed.events),
            ["turn_finished", "agent_session_state"]
        );
        assert!(completed.next_assignments.is_empty());
        let after = stored_session(&store).await;
        assert_eq!(after.public.runtime_status, AgentRuntimeStatus::Idle);
        assert!(!after.public.recovery_required);
        assert_eq!(
            after.public.last_provider_sync_seq,
            mutation.outcome.event.seq
        );
        assert_eq!(after.public.turn_count, 1);
        let error = store
            .mark_provider_turn_running(&start, "retained-result")
            .await
            .err()
            .unwrap_or_else(|| panic!("completed execution must not reopen"));
        assert_rejection_code(&error, "stale_provider_turn");
    }
    Ok(())
}
