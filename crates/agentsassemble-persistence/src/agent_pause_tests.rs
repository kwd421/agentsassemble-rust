use serde_json::json;

use crate::{AgentResidentPlan, AgentResidentRuntime, PersistenceError};

use super::{AGENT_ID, fixture, save_stored_session, stored_session};

#[tokio::test]
async fn resident_pause_and_resume_preserve_identity_queue_and_replay() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let original = stored_session(&store).await;
    let original_identity = runtime_identity(&original);
    let runtime = resident_runtime(&original);

    assert!(matches!(
        store
            .prepare_agent_pause(&principal, "pause-1", &payload)
            .await,
        Ok(AgentResidentPlan::Resident(_))
    ));

    let paused = store
        .execute_agent_pause(&principal, "pause-1", &payload, &runtime)
        .await
        .unwrap_or_else(|error| panic!("pause resident session: {error}"));
    assert_eq!(paused.event.event_type, "agent_session_state");
    assert_eq!(paused.result["agent_session"]["runtime_status"], "paused");
    assert_eq!(paused.result["process_preserved"], true);
    let paused_session = stored_session(&store).await;
    assert!(!paused_session.public.enabled);
    assert_eq!(paused_session.public.runtime_status, "paused");
    assert_eq!(runtime_identity(&paused_session), original_identity);

    let replay = store
        .execute_agent_pause(&principal, "pause-1", &payload, &runtime)
        .await
        .unwrap_or_else(|error| panic!("replay pause: {error}"));
    assert!(replay.deduplicated);
    assert_eq!(replay.event.id, paused.event.id);
    assert!(matches!(
        store
            .execute_agent_pause(
                &principal,
                "pause-1",
                &json!({"participant_id": AGENT_ID}),
                &runtime,
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));

    let queued = store
        .execute_message_with_turn(
            &principal,
            "paused-message",
            "message.send",
            &json!({"content": "@Terra answer only after resume"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue paused message: {error}"));
    assert!(queued.assignments.is_empty());
    let queued_session = stored_session(&store).await;
    assert_eq!(queued_session.pending_inputs.len(), 1);
    assert_eq!(runtime_identity(&queued_session), original_identity);

    let resumed = store
        .resume_paused_agent(&principal, "resume-1", &payload, &runtime)
        .await
        .unwrap_or_else(|error| panic!("resume resident session: {error}"))
        .unwrap_or_else(|| panic!("paused resume must use the resident path"));
    assert_eq!(resumed.result["agent_session"]["runtime_status"], "idle");
    assert_eq!(resumed.result["process_reused"], true);
    let before_assignment = stored_session(&store).await;
    assert!(before_assignment.public.enabled);
    assert_eq!(before_assignment.public.runtime_status, "idle");
    assert_eq!(runtime_identity(&before_assignment), original_identity);

    let assignment = store
        .assign_pending_turn("general")
        .await
        .unwrap_or_else(|error| panic!("assign resumed input: {error}"))
        .unwrap_or_else(|| panic!("resumed pending input must be assigned"));
    assert_eq!(assignment.next_assignments.len(), 1);
    assert!(
        assignment.next_assignments[0]
            .room_view
            .contains("answer only after resume")
    );
    assert_eq!(
        runtime_identity(&assignment.next_assignments[0].session),
        original_identity
    );

    let resume_replay = store
        .resume_paused_agent(&principal, "resume-1", &payload, &runtime)
        .await
        .unwrap_or_else(|error| panic!("replay resident resume: {error}"))
        .unwrap_or_else(|| panic!("resident resume replay must remain state-only"));
    assert!(resume_replay.deduplicated);
    assert_eq!(resume_replay.event.id, resumed.event.id);
}

#[tokio::test]
async fn pause_rejects_incomplete_or_active_runtime_authority() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let mut incomplete = stored_session(&store).await;
    let runtime = resident_runtime(&incomplete);
    incomplete.runtime_lease_token.clear();
    save_stored_session(&store, &incomplete).await;
    assert_invalid_state(
        &store
            .execute_agent_pause(&principal, "pause-incomplete", &payload, &runtime)
            .await,
    );

    let mut active = incomplete;
    active.runtime_lease_token = "lease-generation-1".to_owned();
    active.public.runtime_status = "busy".to_owned();
    save_stored_session(&store, &active).await;
    assert_invalid_state(
        &store
            .execute_agent_pause(&principal, "pause-busy", &payload, &runtime)
            .await,
    );
}

#[tokio::test]
async fn pause_rejects_a_stale_live_runtime_proof_without_mutation() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let original = stored_session(&store).await;
    let mut stale = resident_runtime(&original);
    stale.runtime_lease_token = "different-generation".to_owned();

    assert!(matches!(
        store
            .execute_agent_pause(&principal, "pause-stale", &payload, &stale)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "resident_runtime_changed",
            ..
        })
    ));
    assert_eq!(stored_session(&store).await, original);
}

fn runtime_identity(session: &agentsassemble_domain::DurableAgentSession) -> [&str; 4] {
    [
        &session.provider_session_id,
        &session.runtime_handle_id,
        &session.runtime_owner_id,
        &session.runtime_lease_token,
    ]
}

fn resident_runtime(session: &agentsassemble_domain::DurableAgentSession) -> AgentResidentRuntime {
    AgentResidentRuntime {
        runtime_handle_id: session.runtime_handle_id.clone(),
        runtime_owner_id: session.runtime_owner_id.clone(),
        runtime_lease_token: session.runtime_lease_token.clone(),
        runtime_profile_key: session.runtime_profile_key.clone(),
    }
}

fn assert_invalid_state(result: &Result<crate::CommandOutcome, PersistenceError>) {
    assert!(matches!(
        result,
        Err(PersistenceError::CommandRejected {
            code: "invalid_state",
            ..
        })
    ));
}
