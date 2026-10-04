use super::*;

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[tokio::test]
async fn reconciliation_task_death_releases_ordinary_floor() -> TestResult {
    for dispatched in [false, true] {
        let (store, principal, _directory) = fixture().await;
        three_agents(&store).await;
        let first = store
            .execute_message_with_turn(
                &principal,
                "source",
                "message.send",
                &json!({"content": "An ordinary question"}),
            )
            .await?;
        let assignment = &first.assignments[0];
        let start = store
            .authorize_provider_turn_start(
                "general",
                &assignment.session.public.session_id,
                assignment.turn_generation,
                &assignment.turn_id,
            )
            .await?;
        if dispatched {
            store.mark_provider_turn_running(&start, "running").await?;
        }
        let commit = store
            .record_provider_turn_task_death(
                "general",
                &start.session_id,
                start.turn_generation,
                &start.execution_id,
            )
            .await?;
        assert_eq!(commit.next_assignments.len(), 1);
        assert_ne!(
            commit.next_assignments[0].session.public.session_id,
            start.session_id
        );
        assert_eq!(
            commit.next_assignments[0].session.active_source_event_id,
            first.outcome.event.id
        );
    }
    Ok(())
}

#[tokio::test]
async fn interrupt_unissued_recovery_releases_floor() -> TestResult {
    interrupt_recovery(0).await
}

#[tokio::test]
async fn interrupt_quiescence_recovery_releases_floor() -> TestResult {
    interrupt_recovery(1).await
}

#[tokio::test]
async fn interrupt_ambiguous_recovery_releases_floor() -> TestResult {
    interrupt_recovery(2).await
}

#[tokio::test]
async fn interrupt_task_death_releases_floor() -> TestResult {
    interrupt_recovery(3).await
}

async fn interrupt_recovery(stage: u8) -> TestResult {
    let (store, principal, _directory) = fixture().await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "An ordinary question"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "interrupt").await;
    three_agents(&store).await;
    let mutation = store
        .execute_agent_interrupt(
            TrustedPrincipal(&principal),
            "interrupt",
            &json!({"agent_id": AGENT_ID}),
        )
        .await?;
    let effect = mutation.host_interrupt_effect.ok_or("missing interrupt")?;
    let claim = store
        .claim_provider_turn_interrupt(&effect, "10000000-0000-4000-8000-000000000099")
        .await?;
    if stage != 0 {
        let dispatch = store.authorize_provider_interrupt_dispatch(&claim).await?;
        if stage == 1 {
            let waiting = store.mark_provider_interrupt_issued(&dispatch).await?;
            store
                .mark_provider_interrupt_recovery_required(&waiting)
                .await?;
        } else if stage == 2 {
            store.mark_provider_interrupt_ambiguous(&dispatch).await?;
        } else {
            store
                .record_provider_turn_task_death(
                    "general",
                    AGENT_ID,
                    start.turn_generation,
                    &start.execution_id,
                )
                .await?;
        }
    } else {
        store
            .handoff_unissued_provider_interrupt_claim(&claim)
            .await?;
    }
    assert!(stored_session(&store).await.public.recovery_required);
    let receipt: Option<String> = sqlx::query_scalar(
        "SELECT released_input_ids FROM provider_turn_executions WHERE execution_id = ?",
    )
    .bind(&start.execution_id)
    .fetch_one(&store.pool)
    .await?;
    assert_eq!(
        serde_json::from_str::<Vec<String>>(&receipt.ok_or("missing receipt")?)?,
        vec![first.outcome.event.id]
    );
    store
        .execute_message_with_turn(
            &principal,
            "new",
            "message.send",
            &json!({"content": "Another ordinary question"}),
        )
        .await?;
    assert!(stored_session(&store).await.pending_inputs.is_empty());
    Ok(())
}

#[tokio::test]
async fn startup_repairs_upgraded_quarantine_and_confirmed_stop_receipts() -> TestResult {
    for stop_stage in [0, 1, 2] {
        let (store, principal, directory) = fixture().await;
        let first = store
            .execute_message_with_turn(
                &principal,
                "source",
                "message.send",
                &json!({"content": "An ordinary question"}),
            )
            .await?;
        let start = running_authority(&store, &first.assignments[0], "legacy").await;
        if stop_stage != 0 {
            let payload = json!({"agent_id": AGENT_ID});
            let crate::AgentStopPlan::Stop(plan) = store
                .prepare_agent_stop(TrustedPrincipal(&principal), "stop", &payload)
                .await?
            else {
                panic!("stop")
            };
            store
                .authorize_agent_stop_effect(
                    TrustedPrincipal(&principal),
                    "stop",
                    &payload,
                    &plan.operation_id,
                )
                .await?;
            store
                .record_agent_stop_effect("general", AGENT_ID, &plan.operation_id)
                .await?;
            if stop_stage == 2 {
                store
                    .finalize_agent_stop(&principal, "stop", &payload)
                    .await?;
            }
        } else {
            store
                .mark_provider_turn_recovery_required(&start, None)
                .await?;
        }
        three_agents(&store).await;
        sqlx::query("ALTER TABLE provider_turn_executions DROP COLUMN released_input_ids")
            .execute(&store.pool)
            .await?;
        sqlx::query("UPDATE runtime_metadata SET value = '77' WHERE key = 'schema_version'")
            .execute(&store.pool)
            .await?;
        store.pool.close().await;
        drop(store);
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
        let page = store.load_provider_turn_reconciliation_page(None).await?;
        let next = page
            .candidates
            .iter()
            .find(|candidate| {
                candidate.execution.phase == crate::ProviderTurnExecutionPhase::Assigned
            })
            .ok_or("startup left floor stalled")?;
        assert_ne!(next.session.public.session_id, AGENT_ID);
        assert_eq!(next.session.active_source_event_id, first.outcome.event.id);
        if stop_stage == 0 {
            let replay = store
                .mark_provider_turn_recovery_required(&start, None)
                .await?;
            assert!(replay.events.is_empty());
            assert!(replay.next_assignments.is_empty());
        }
        assert!(store.assign_pending_turn("general").await?.is_none());
    }
    Ok(())
}

#[tokio::test]
async fn recovery_without_receipt_keeps_floor_until_replay_repairs_it() -> TestResult {
    let (store, principal, _directory) = fixture().await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "source",
            "message.send",
            &json!({"content": "An ordinary question"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "legacy").await;
    store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    sqlx::query("UPDATE provider_turn_executions SET released_input_ids = NULL")
        .execute(&store.pool)
        .await?;
    three_agents(&store).await;
    let next = store
        .execute_message_with_turn(
            &principal,
            "next",
            "message.send",
            &json!({"content": "@Flash a separate question"}),
        )
        .await?;
    assert!(next.assignments.is_empty(), "phase alone released floor");
    let repaired = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert_eq!(repaired.next_assignments.len(), 1);
    assert!(
        input_ids(&repaired.next_assignments[0].session.inflight_inputs)
            .contains(&first.outcome.event.id)
    );
    Ok(())
}
