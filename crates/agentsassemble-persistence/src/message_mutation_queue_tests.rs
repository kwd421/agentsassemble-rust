use agentsassemble_domain::{RoomInputDeliveryKind, RoomSettings, public_settings};
use serde_json::json;

use super::{AGENT_ID, authority, event_types, fixture, running_authority, stored_session};

#[tokio::test]
async fn deletion_removes_only_pending_ordered_and_ambient_inputs_atomically() {
    let (store, principal, _directory) = fixture().await;
    let active = store
        .execute_message_with_turn(
            &principal,
            "delete-queue-active",
            "message.send",
            &json!({"content": "@Terra hold the active provider turn"}),
        )
        .await
        .unwrap_or_else(|error| panic!("start active provider turn: {error}"));
    let assignment = active
        .assignments
        .first()
        .unwrap_or_else(|| panic!("active message must assign Terra"));
    let start = running_authority(&store, assignment, "delete-queue-provider-turn").await;
    let ordered = store
        .execute_message_with_turn(
            &principal,
            "delete-queue-ordered",
            "message.send",
            &json!({"content": "@Terra remove this queued ordered input"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue ordered input: {error}"));
    assert!(ordered.assignments.is_empty());

    let revision = public_settings(&RoomSettings::defaults("General"))
        .unwrap_or_else(|error| panic!("read initial settings revision: {error}"))
        .settings_revision;
    store
        .execute_room_settings_update(
            &principal,
            "delete-queue-ambient-mode",
            &json!({"expected_revision": revision, "conversation_mode": "ambient"}),
        )
        .await
        .unwrap_or_else(|error| panic!("switch to ambient: {error}"));
    let ambient = store
        .execute_message_with_turn(
            &principal,
            "delete-queue-ambient",
            "message.send",
            &json!({"content": "remove this queued ambient input"}),
        )
        .await
        .unwrap_or_else(|error| panic!("queue ambient input: {error}"));
    assert!(ambient.assignments.is_empty());
    let queued = stored_session(&store).await;
    assert_eq!(
        queued
            .pending_inputs
            .iter()
            .map(|input| (&input.event_id, input.delivery_kind))
            .collect::<Vec<_>>(),
        [
            (
                &ordered.outcome.event.id,
                RoomInputDeliveryKind::OrderedObservation,
            ),
            (
                &ambient.outcome.event.id,
                RoomInputDeliveryKind::AmbientObservation,
            ),
        ]
    );

    assert_delete_rollback_preserves_queue(&store, &principal, &ordered.outcome.event.id).await;

    for (request_id, event_id) in [
        ("delete-queue-ordered-commit", &ordered.outcome.event.id),
        ("delete-queue-ambient-commit", &ambient.outcome.event.id),
    ] {
        delete_pending_input(&store, &principal, request_id, event_id).await;
    }
    assert!(stored_session(&store).await.pending_inputs.is_empty());

    let commit = store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "delete-queue-provider-turn", None),
            "The active provider turn still completes once.",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("complete provider turn after deletions: {error}"));
    assert_eq!(
        event_types(&commit.events),
        ["message_final", "turn_finished", "agent_session_state"]
    );
    assert!(commit.next_assignments.is_empty());
    let final_session = stored_session(&store).await;
    assert_eq!(final_session.public.status, "attached");
    assert_eq!(final_session.public.runtime_status, "idle");
    assert!(final_session.pending_inputs.is_empty());
}

async fn assert_delete_rollback_preserves_queue(
    store: &crate::SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    event_id: &str,
) {
    sqlx::query(
        "CREATE TRIGGER reject_queued_delete_result BEFORE INSERT ON command_results WHEN NEW.action = 'message.delete' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("install queued-delete rollback trigger: {error}"));
    assert!(
        store
            .execute_message_mutation(
                principal,
                "delete-queue-ordered-failed",
                "message.delete",
                &json!({"event_id": event_id}),
            )
            .await
            .is_err()
    );
    assert_eq!(stored_session(store).await.pending_inputs.len(), 2);
    sqlx::query("DROP TRIGGER reject_queued_delete_result")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("remove queued-delete rollback trigger: {error}"));
}

async fn delete_pending_input(
    store: &crate::SqliteStore,
    principal: &agentsassemble_domain::AuthenticatedPrincipal,
    request_id: &str,
    event_id: &str,
) {
    store
        .execute_message_mutation(
            principal,
            request_id,
            "message.delete",
            &json!({"event_id": event_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("delete pending input {event_id}: {error}"));
}
