use super::*;
use crate::RoomMutationAuthority::TrustedPrincipal;

async fn three_agents(store: &SqliteStore) {
    for (id, name) in [(SECOND_AGENT_ID, "Flash"), (SPEAKER_AGENT_ID, "Worker")] {
        let now = Utc::now();
        let mut session = attached_session(now);
        session.public.session_id = id.to_owned();
        session.public.participant_id = id.to_owned();
        session.public.display_name = name.to_owned();
        session.runtime_handle_id = format!("runtime-{name}");
        session.provider_session_id = format!("thread-{name}");
        insert_agent(
            store,
            &participant(id, name, "agent", ParticipantRole::Agent, now),
            &session,
        )
        .await;
    }
}

async fn stop(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    id: &str,
) -> crate::RoomCommandMutation {
    let payload = json!({"agent_id": id});
    let crate::AgentStopPlan::Stop(plan) = store
        .prepare_agent_stop(TrustedPrincipal(principal), "stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"))
    else {
        panic!("stop plan")
    };
    store
        .authorize_agent_stop_effect(
            TrustedPrincipal(principal),
            "stop",
            &payload,
            &plan.operation_id,
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    store
        .record_agent_stop_effect("general", id, &plan.operation_id)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    store
        .record_agent_stop_effect("general", id, &plan.operation_id)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    store
        .finalize_agent_stop(TrustedPrincipal(principal), "stop", &payload)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"))
}

#[tokio::test]
async fn ordered_quarantine_hands_off_once_and_late_success_does_not_publish() {
    let (store, principal, directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "ordinary",
            "message.send",
            &json!({"content": "Discuss this ordinary message"}),
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    let assignment = &first.assignments[0];
    let start = running_authority(&store, assignment, "late-result").await;
    let quarantined = store
        .mark_provider_turn_recovery_required(&start, None)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert_eq!(quarantined.next_assignments.len(), 1);
    assert_ne!(
        quarantined.next_assignments[0].session.public.session_id,
        start.session_id
    );
    assert_eq!(
        quarantined.next_assignments[0]
            .session
            .active_source_event_id,
        first.outcome.event.id
    );
    let next = quarantined.next_assignments[0].clone();
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    let replay = store
        .mark_provider_turn_recovery_required(&start, None)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert!(replay.events.is_empty());
    assert!(replay.next_assignments.is_empty());
    let late = store
        .complete_agent_turn(
            "general",
            &start.session_id,
            authority(&start, "late-result", None),
            "DUPLICATE LATE ANSWER",
            "",
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert!(!event_types(&late.events).contains(&"message_final"));
    assert!(late.next_assignments.is_empty());
    let execution = store
        .provider_turn_execution("general", &start.session_id, start.turn_generation)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert_eq!(
        execution.phase,
        crate::ProviderTurnExecutionPhase::Completed
    );
    let next_start = running_authority(&store, &next, "next-result").await;
    let completed = store
        .complete_agent_turn(
            "general",
            &next_start.session_id,
            authority(&next_start, "next-result", None),
            "@Host the answer",
            "",
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert_eq!(
        completed
            .events
            .iter()
            .filter(|event| event.event_type == "message_final")
            .count(),
        1
    );
}

#[tokio::test]
async fn ordered_confirmed_stop_hands_off_ordinary_but_preserves_addressed_input() {
    for addressed in [false, true] {
        let (store, principal, _directory) = fixture().await;
        three_agents(&store).await;
        let content = if addressed {
            "@Terra answer this"
        } else {
            "Discuss this ordinary message"
        };
        let first = store
            .execute_message_with_turn(
                &principal,
                "input",
                "message.send",
                &json!({"content": content}),
            )
            .await
            .unwrap_or_else(|error| panic!("test operation: {error}"));
        let assignment = &first.assignments[0];
        let start = running_authority(&store, assignment, "stop-result").await;
        let stopped = stop(&store, &principal, &start.session_id).await;
        assert_eq!(stopped.assignments.len(), usize::from(!addressed));
        let mut tx = store
            .pool
            .begin()
            .await
            .unwrap_or_else(|error| panic!("test operation: {error}"));
        let session = crate::agent_lifecycle::load_session(&mut tx, "general", &start.session_id)
            .await
            .unwrap_or_else(|error| panic!("test operation: {error}"));
        tx.commit()
            .await
            .unwrap_or_else(|error| panic!("test operation: {error}"));
        assert_eq!(session.pending_inputs.len(), usize::from(addressed));
        assert!(session.public.active_turn_id.is_empty());
        let replay = store
            .finalize_agent_stop(
                TrustedPrincipal(&principal),
                "stop",
                &json!({"agent_id": start.session_id}),
            )
            .await
            .unwrap_or_else(|error| panic!("test operation: {error}"));
        assert!(replay.assignments.is_empty());
    }
}

#[tokio::test]
async fn ordered_addressed_quarantine_releases_floor_without_handoff() {
    let (store, principal, _directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "direct",
            "message.send",
            &json!({"content": "@Terra answer this"}),
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    let start = running_authority(&store, &first.assignments[0], "direct-result").await;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert!(quarantine.next_assignments.is_empty());
    let second = store
        .execute_message_with_turn(
            &principal,
            "other",
            "message.send",
            &json!({"content": "@Flash unrelated question"}),
        )
        .await
        .unwrap_or_else(|error| panic!("test operation: {error}"));
    assert_eq!(second.assignments.len(), 1);
    assert_eq!(
        second.assignments[0].session.public.session_id,
        SECOND_AGENT_ID
    );
    let stopped = stop(&store, &principal, AGENT_ID).await;
    assert!(stopped.assignments.is_empty());
    assert_eq!(stored_session(&store).await.pending_inputs.len(), 1);
}

#[tokio::test]
async fn ordered_recovery_stop_resume_uses_fresh_generation_without_reusing_handoff()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "input",
            "message.send",
            &json!({"content": "Discuss this ordinary message"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "resume-result").await;
    let payload = json!({"agent_id": start.session_id});
    let quarantined = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    let rejected = store
        .prepare_agent_resume(TrustedPrincipal(&principal), "too-early", &payload)
        .await;
    assert!(
        matches!(rejected, Err(PersistenceError::CommandRejected { code, .. }) if code == "provider_turn_recovery_required")
    );
    stop(&store, &principal, &start.session_id).await;
    resume_stopped(&store, &principal, &payload).await?;
    assert!(store.assign_pending_turn("general").await?.is_none());
    // Retire the next speaker with runtime-gone proof, without routing a new public message.
    let next = &quarantined.next_assignments[0];
    let next_start = running_authority(&store, next, "next").await;
    store
        .fail_agent_turn(
            "general",
            &next_start.session_id,
            authority(&next_start, "next", None),
            "provider_runtime_gone",
            "gone",
            Some((
                &next_start.runtime_handle_id,
                &next_start.runtime_owner_id,
                &next_start.runtime_lease_token,
            )),
        )
        .await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    let fresh = store
        .execute_message_with_turn(
            &principal,
            "fresh",
            "message.send",
            &json!({"content": format!("@{} new question", start.session_id)}),
        )
        .await?;
    assert_eq!(fresh.assignments.len(), 1);
    assert_eq!(
        fresh.assignments[0].turn_generation,
        start.turn_generation + 1
    );
    assert_eq!(
        input_ids(&fresh.assignments[0].session.inflight_inputs),
        vec![fresh.outcome.event.id]
    );
    assert!(
        store
            .complete_agent_turn(
                "general",
                &start.session_id,
                authority(&start, "resume-result", None),
                "stale answer",
                "",
                None
            )
            .await
            .is_err()
    );
    Ok(())
}

#[tokio::test]
async fn ordered_recovery_schema_upgrade_preserves_exact_execution_and_input()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, directory) = fixture().await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "input",
            "message.send",
            &json!({"content": "@Terra keep this input"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "retained").await;
    let before = stored_session(&store).await;
    let execution = store
        .provider_turn_execution("general", AGENT_ID, start.turn_generation)
        .await?;
    sqlx::query("ALTER TABLE provider_turn_executions DROP COLUMN released_input_ids")
        .execute(&store.pool)
        .await?;
    crate::member_schema::restore_v80_fixture(&store).await?;
    sqlx::query("UPDATE runtime_metadata SET value = '77' WHERE key = 'schema_version'")
        .execute(&store.pool)
        .await?;
    store.pool.close().await;
    drop(store);
    let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3")).await?;
    assert_eq!(
        serde_json::to_value(stored_session(&store).await)?,
        serde_json::to_value(before)?
    );
    assert_eq!(
        store
            .provider_turn_execution("general", AGENT_ID, start.turn_generation)
            .await?,
        execution
    );
    assert!(
        store
            .mark_provider_turn_recovery_required(&start, None)
            .await?
            .next_assignments
            .is_empty()
    );
    Ok(())
}

async fn resume_stopped(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    payload: &serde_json::Value,
) -> Result<(), PersistenceError> {
    let crate::AgentStartPlan::Start(resume) = store
        .prepare_agent_resume(TrustedPrincipal(principal), "resume", payload)
        .await?
    else {
        panic!("resume launch")
    };
    assert!(resume.session.pending_inputs.is_empty());
    let runtime = crate::AgentRuntimeStarted {
        runtime_handle_id: "resumed-runtime".to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        runtime_lease_token: "resumed-lease".to_owned(),
        provider_session_id: resume.session.provider_session_id.clone(),
        runtime_reused: false,
        provider_session_reused: true,
        provider_session_active: true,
    };
    store
        .authorize_agent_start_effect(
            TrustedPrincipal(principal),
            "resume",
            payload,
            &resume.operation_id,
            "agent.resume",
            &runtime.runtime_handle_id,
            &runtime.runtime_owner_id,
            &runtime.runtime_lease_token,
        )
        .await?;
    store
        .complete_agent_resume(
            TrustedPrincipal(principal),
            "resume",
            payload,
            &resume.operation_id,
            &runtime,
        )
        .await?;
    Ok(())
}

#[tokio::test]
async fn ordered_recovery_mixed_observation_keeps_addressed_input_after_late_success()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "initial",
            "message.send",
            &json!({"content": "@Terra initial"}),
        )
        .await?;
    store
        .execute_message_with_turn(
            &principal,
            "ordinary",
            "message.send",
            &json!({"content": "Discuss an ordinary question"}),
        )
        .await?;
    let direct = store
        .execute_message_with_turn(
            &principal,
            "direct",
            "message.send",
            &json!({"content": "@Terra private responsibility"}),
        )
        .await?;
    let initial = running_authority(&store, &first.assignments[0], "initial").await;
    let batch = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&initial, "initial", None),
            "Initial answer",
            "",
            None,
        )
        .await?;
    assert_eq!(batch.next_assignments[0].session.inflight_inputs.len(), 2);
    three_agents(&store).await;
    let start = running_authority(&store, &batch.next_assignments[0], "mixed").await;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert_eq!(quarantine.next_assignments.len(), 1);
    let late = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "mixed", None),
            "Mixed late answer",
            "",
            None,
        )
        .await?;
    assert!(!event_types(&late.events).contains(&"message_final"));
    assert_eq!(
        input_ids(&stored_session(&store).await.pending_inputs),
        vec![direct.outcome.event.id]
    );
    Ok(())
}

#[tokio::test]
async fn ordered_quarantine_after_archive_retains_custody_without_scheduling()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = fixture().await;
    three_agents(&store).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "input",
            "message.send",
            &json!({"content": "An ordinary question"}),
        )
        .await?;
    let start = running_authority(&store, &first.assignments[0], "archive-race").await;
    let uid = store.snapshot("general", 0, 200).await?.room.room_uid;
    store
        .execute_room_lifecycle(
            TrustedPrincipal(&principal),
            "archive",
            "room.archive",
            &json!({"room_uid": uid, "archived": true}),
        )
        .await?;
    let quarantine = store
        .mark_provider_turn_recovery_required(&start, None)
        .await?;
    assert_eq!(quarantine.events.len(), 1);
    assert!(quarantine.next_assignments.is_empty());
    assert_eq!(
        store
            .provider_turn_execution("general", &start.session_id, start.turn_generation)
            .await?
            .phase,
        crate::ProviderTurnExecutionPhase::RecoveryRequired
    );
    Ok(())
}

#[path = "ordered_release_entry_tests.rs"]
mod entry_tests;

#[path = "ordered_target_identity_tests.rs"]
mod target_tests;
