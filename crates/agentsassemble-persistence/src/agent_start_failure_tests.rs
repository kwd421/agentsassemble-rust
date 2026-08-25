use agentsassemble_domain::{AgentSession, AuthenticatedPrincipal};
use serde_json::{Value, json};

use super::{AgentRuntimeStarted, AgentStartPlan};
use crate::{
    AgentLaunchFailureCommit, PersistenceError, SqliteStore,
    agent_lifecycle::tests::{AGENT_ID, fixture},
};

#[tokio::test]
async fn stale_completion_fails_closed_and_safe_failure_replays() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"agent_id": AGENT_ID});
    let AgentStartPlan::Start(effect) = store
        .prepare_agent_start(&principal, "start-failed", &payload)
        .await
        .unwrap_or_else(|error| panic!("prepare start: {error}"))
    else {
        panic!("stopped session must require a start effect");
    };
    let stale = store
        .complete_agent_start(
            &principal,
            "start-failed",
            &payload,
            "different-operation",
            &AgentRuntimeStarted {
                runtime_handle_id: "owned-runtime".to_owned(),
                runtime_owner_id: "supervisor-instance-1".to_owned(),
                runtime_lease_token: "lease-generation-1".to_owned(),
                provider_session_id: "provider-thread".to_owned(),
                runtime_reused: false,
                provider_session_reused: false,
                provider_session_active: true,
            },
        )
        .await;
    assert!(matches!(
        stale,
        Err(PersistenceError::CommandRejected {
            code: "stale_start_confirmation",
            ..
        })
    ));
    store
        .authorize_agent_start_effect(
            &principal,
            "start-failed",
            &payload,
            &effect.operation_id,
            "agent.start",
            "owned-runtime",
            "supervisor-instance-1",
            "lease-generation-1",
        )
        .await
        .unwrap_or_else(|error| panic!("authorize failed start effect: {error}"));
    let failure = store
        .fail_agent_start(
            &principal,
            "start-failed",
            &payload,
            &effect.operation_id,
            "runtime_start_failed",
            "/Users/alice/private/bin/codex:\nAuthorization: Bearer secret-provider-token",
        )
        .await
        .unwrap_or_else(|error| panic!("record start failure: {error}"));
    assert_eq!(failure.events[0].event_type, "error");
    assert_eq!(
        failure.events[0].content.as_deref(),
        Some("[local path]\n[redacted]")
    );
    assert_eq!(failure.code, "runtime_start_failed");
    assert_eq!(failure.message, "[local path]\n[redacted]");
    assert_terminal_start_failure(&store, &principal, &payload, &failure).await;
    assert!(matches!(
        store
            .prepare_agent_stop(&principal, "stop-after-failed-start", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "runtime_handle_unavailable",
            ..
        })
    ));
    assert!(matches!(
        store
            .prepare_agent_start(&principal, "new-start-after-safe-failure", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare replacement start: {error}")),
        AgentStartPlan::Start(_)
    ));
}

async fn assert_terminal_start_failure(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    payload: &Value,
    failure: &AgentLaunchFailureCommit,
) {
    let snapshot = store
        .snapshot("general", 0, 200)
        .await
        .unwrap_or_else(|error| panic!("snapshot failed start: {error}"));
    assert_failed_projection(&snapshot.agent_sessions[0]);
    let reservation = sqlx::query_as::<_, (String, String, String)>(
        "SELECT status, failure_code, failure_message FROM lifecycle_command_reservations WHERE room_id = 'general' AND request_id = 'start-failed'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("inspect failed-start reservations: {error}"));
    assert_eq!(
        reservation,
        (
            "rejected".to_owned(),
            failure.code.clone(),
            failure.message.clone()
        )
    );
    assert!(matches!(
        store
            .prepare_agent_start(principal, "start-failed", payload)
            .await,
        Err(PersistenceError::StoredCommandRejected { code, message })
            if code == failure.code && message == failure.message
    ));
    assert!(
        store
            .load_runtime_reconciliation_candidates()
            .await
            .unwrap_or_else(|error| panic!("load post-failure candidates: {error}"))
            .is_empty()
    );
}

fn assert_failed_projection(session: &AgentSession) {
    assert_eq!(session.runtime_status, "error");
    assert_eq!(session.last_error_code, "runtime_start_failed");
    assert_eq!(session.last_error, "[local path]\n[redacted]");
    assert!(!session.last_error.contains("alice"));
    assert!(!session.last_error.contains("secret-provider-token"));
    assert!(!session.enabled);
}
