use serde_json::json;

use super::{AgentRuntimeStarted, AgentStartPlan, AgentStopPlan};
use crate::agent_lifecycle::tests::{AGENT_ID, fixture};

#[tokio::test]
async fn stopped_resume_reuses_durable_provider_session_and_replays_as_resume() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(start) = store
        .prepare_agent_start(&principal, "initial-start", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare initial start: {error}"))
    else {
        panic!("stopped session must require initial launch");
    };
    store
        .authorize_agent_start_effect(
            &principal,
            "initial-start",
            &payload,
            &start.operation_id,
            "agent.start",
            "runtime-before-stop",
            "supervisor-instance-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize initial start: {error}"));
    store
        .complete_agent_start(
            &principal,
            "initial-start",
            &payload,
            &start.operation_id,
            &started("runtime-before-stop", false),
        )
        .await
        .unwrap_or_else(|error| panic!("complete initial start: {error}"));

    let stop_request_id = "stop-before-resume";
    let AgentStopPlan::Stop(stop) = store
        .prepare_agent_stop(&principal, stop_request_id, &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare stop: {error}"))
    else {
        panic!("running session must require exact stop effect");
    };
    store
        .authorize_agent_stop_effect(&principal, stop_request_id, &payload, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("authorize stop: {error}"));
    store
        .record_agent_stop_effect("general", AGENT_ID, &stop.operation_id)
        .await
        .unwrap_or_else(|error| panic!("record stop: {error}"));
    store
        .finalize_agent_stop(&principal, stop_request_id, &payload)
        .await
        .unwrap_or_else(|error| panic!("finalize stop: {error}"));

    let AgentStartPlan::Start(resume) = store
        .prepare_agent_resume(&principal, "resume-stopped", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare resume: {error}"))
    else {
        panic!("stopped session must require a resume launch effect");
    };
    assert_eq!(resume.session.provider_session_id, "provider-thread-stable");
    store
        .authorize_agent_start_effect(
            &principal,
            "resume-stopped",
            &payload,
            &resume.operation_id,
            "agent.resume",
            "runtime-after-resume",
            "supervisor-instance-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize resume: {error}"));
    let outcome = store
        .complete_agent_resume(
            &principal,
            "resume-stopped",
            &payload,
            &resume.operation_id,
            &started("runtime-after-resume", true),
        )
        .await
        .unwrap_or_else(|error| panic!("complete resume: {error}"));
    let resumed = &outcome.result["agent_session"];
    assert_eq!(resumed["runtime_status"], "idle");
    assert_eq!(resumed["provider_session_reused"], true);

    let AgentStartPlan::Outcome(replayed) = store
        .prepare_agent_resume(&principal, "resume-stopped", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay resume: {error}"))
    else {
        panic!("completed resume must not launch twice");
    };
    assert!(replayed.deduplicated);
    let action = sqlx::query_scalar::<_, String>(
        "SELECT action FROM command_results WHERE room_id = 'general' AND request_id = 'resume-stopped'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read resume command result: {error}"));
    assert_eq!(action, "agent.resume");
}

fn started(runtime_handle_id: &str, provider_session_reused: bool) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: runtime_handle_id.to_owned(),
        runtime_owner_id: "supervisor-instance-1".to_owned(),
        provider_session_id: "provider-thread-stable".to_owned(),
        runtime_reused: false,
        provider_session_reused,
        provider_session_active: true,
    }
}
