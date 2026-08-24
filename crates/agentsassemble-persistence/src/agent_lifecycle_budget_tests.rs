use serde_json::json;

use crate::AgentStopPlan;

use super::{
    load_session, save_session,
    tests::{AGENT_ID, fixture},
};

#[tokio::test]
async fn only_a_stop_that_owns_runtime_cleanup_bypasses_write_budgets() {
    let payload = json!({"agent_id": AGENT_ID});
    let (stopped_store, stopped_principal, _stopped_directory) = fixture().await;
    assert!(
        stopped_store
            .command_requires_principal_budget(
                &stopped_principal,
                "stopped-noop",
                "agent.stop",
                &payload,
            )
            .await
            .unwrap_or_else(|error| panic!("classify stopped no-op: {error}"))
    );
    assert!(matches!(
        stopped_store
            .prepare_agent_stop(&stopped_principal, "stopped-noop", &payload)
            .await
            .unwrap_or_else(|error| panic!("stop stopped session: {error}")),
        AgentStopPlan::Outcome(_)
    ));
    let stopped_budget = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(command_count), 0) FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&stopped_store.pool)
    .await
    .unwrap_or_else(|error| panic!("read stopped-session budget: {error}"));
    assert_eq!(stopped_budget, 1);

    let (running_store, running_principal, _running_directory) = fixture().await;
    let mut transaction = running_store
        .pool
        .begin()
        .await
        .unwrap_or_else(|error| panic!("begin running-session update: {error}"));
    let mut session = load_session(&mut transaction, "general", AGENT_ID)
        .await
        .unwrap_or_else(|error| panic!("load running session: {error}"));
    session.public.runtime_status = "idle".to_owned();
    session.public.enabled = true;
    session.runtime_handle_id = "owned-runtime".to_owned();
    session.runtime_owner_id = "owned-supervisor".to_owned();
    save_session(&mut transaction, &session)
        .await
        .unwrap_or_else(|error| panic!("save running session: {error}"));
    transaction
        .commit()
        .await
        .unwrap_or_else(|error| panic!("commit running session: {error}"));
    assert!(
        !running_store
            .command_requires_principal_budget(
                &running_principal,
                "owned-cleanup",
                "agent.stop",
                &payload,
            )
            .await
            .unwrap_or_else(|error| panic!("classify owned cleanup: {error}"))
    );
    assert!(matches!(
        running_store
            .prepare_agent_stop(&running_principal, "owned-cleanup", &payload)
            .await
            .unwrap_or_else(|error| panic!("prepare owned cleanup: {error}")),
        AgentStopPlan::Stop(_)
    ));
    let running_budget = sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(SUM(command_count), 0) FROM room_write_budgets WHERE room_id = 'general'",
    )
    .fetch_one(&running_store.pool)
    .await
    .unwrap_or_else(|error| panic!("read cleanup budget: {error}"));
    assert_eq!(running_budget, 0);
}
