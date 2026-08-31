use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, RoomEvent,
};
use serde_json::json;

use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn edit_replaces_history_search_and_pin_once_without_floor_work() {
    let (store, operator) = fixture().await;
    let sent = store
        .execute_message_with_turn(
            &operator,
            "send-edit-target",
            "message.send",
            &json!({"content": "originalneedle"}),
        )
        .await
        .unwrap_or_else(|error| panic!("send edit target: {error}"));
    let target_id = sent.outcome.event.id;
    store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &target_id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin edit target: {error}"));

    let payload = json!({"event_id": target_id, "content": "revisedneedle"});
    let edited = store
        .execute_message_mutation(&operator, "edit-target", "message.edit", &payload)
        .await
        .unwrap_or_else(|error| panic!("edit target: {error}"));
    assert_eq!(edited.event.event_type, "message_updated");
    assert_eq!(edited.event.extra["target_event_id"], target_id);
    assert_eq!(search_count(&store, "originalneedle").await, 0);
    assert_eq!(search_count(&store, "revisedneedle").await, 1);
    let pins = store
        .local_lobby_message_pins(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
        )
        .await
        .unwrap_or_else(|error| panic!("read edited pin: {error}"));
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].content, "revisedneedle");
    let stored = stored_event(&store, &target_id).await;
    assert_eq!(stored.content.as_deref(), Some("revisedneedle"));
    assert!(stored.extra.contains_key("edited_at"));

    let before_replay = event_count(&store).await;
    let replay = store
        .execute_message_mutation(&operator, "edit-target", "message.edit", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay edit: {error}"));
    assert!(replay.deduplicated);
    assert_eq!(event_count(&store).await, before_replay);
    assert!(matches!(
        store
            .execute_message_mutation(
                &operator,
                "edit-target",
                "message.edit",
                &json!({"event_id": target_id, "content": "changed replay"}),
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));
    assert_rejected(
        store
            .execute_message_mutation(
                &operator,
                "empty-edit",
                "message.edit",
                &json!({"event_id": target_id, "content": "  "}),
            )
            .await,
        "empty",
    );
}

#[tokio::test]
async fn delete_rolls_back_then_removes_exact_attachment_search_and_pin() {
    let (store, operator) = fixture().await;
    let attachment = store
        .store_message_attachment(
            &operator,
            "evidence.txt",
            "text/plain",
            b"evidence".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store delete attachment: {error}"));
    let sent = store
        .execute_message_with_turn(
            &operator,
            "send-delete-target",
            "message.send",
            &json!({"content": "deleteneedle", "attachment_ids": [attachment.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("send delete target: {error}"));
    let target_id = sent.outcome.event.id;
    store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &target_id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin delete target: {error}"));
    sqlx::query(
        "CREATE TRIGGER reject_message_delete_result BEFORE INSERT ON command_results WHEN NEW.action = 'message.delete' BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
    )
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("install delete rollback trigger: {error}"));
    let payload = json!({"event_id": target_id});
    assert!(
        store
            .execute_message_mutation(&operator, "delete-target", "message.delete", &payload)
            .await
            .is_err()
    );
    assert_eq!(search_count(&store, "deleteneedle").await, 1);
    assert_eq!(pin_count(&store).await, 1);
    assert_eq!(stored_attachment_count(&store, &attachment.id).await, 1);
    assert_eq!(
        stored_event(&store, &target_id)
            .await
            .extra
            .get("message_deleted"),
        None
    );

    sqlx::query("DROP TRIGGER reject_message_delete_result")
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("remove delete rollback trigger: {error}"));
    let deleted = store
        .execute_message_mutation(&operator, "delete-target", "message.delete", &payload)
        .await
        .unwrap_or_else(|error| panic!("delete target: {error}"));
    assert_eq!(deleted.event.event_type, "message_deleted");
    assert_eq!(deleted.result["attachment_ids"], json!([attachment.id]));
    assert_eq!(search_count(&store, "deleteneedle").await, 0);
    assert_eq!(pin_count(&store).await, 0);
    assert_eq!(stored_attachment_count(&store, &attachment.id).await, 0);
    let tombstone = stored_event(&store, &target_id).await;
    assert_eq!(tombstone.content.as_deref(), Some(""));
    assert_eq!(tombstone.extra["attachments"], json!([]));
    assert_eq!(tombstone.extra["message_deleted"], true);

    let before_replay = event_count(&store).await;
    let replay = store
        .execute_message_mutation(&operator, "delete-target", "message.delete", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay delete: {error}"));
    assert!(replay.deduplicated);
    assert_eq!(event_count(&store).await, before_replay);
}

#[tokio::test]
async fn poll_delete_removes_projection_and_redacts_every_ballot_transition() {
    let (store, operator) = fixture().await;
    let created = store
        .execute_message_with_turn(
            &operator,
            "create-delete-poll",
            "message.send",
            &json!({
                "kind": "vote",
                "vote_question": "pollsecretneedle",
                "vote_options": ["Yes", "No"]
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("create delete poll: {error}"));
    let vote_id = created.outcome.event.id;
    store
        .execute_message_with_turn(
            &operator,
            "cast-delete-poll",
            "message.send",
            &json!({"kind": "vote_cast", "vote_id": vote_id, "vote_choice": "Yes"}),
        )
        .await
        .unwrap_or_else(|error| panic!("cast delete poll: {error}"));

    store
        .execute_message_mutation(
            &operator,
            "delete-poll",
            "message.delete",
            &json!({"event_id": vote_id}),
        )
        .await
        .unwrap_or_else(|error| panic!("delete poll: {error}"));
    assert_eq!(search_count(&store, "pollsecretneedle").await, 0);
    assert_eq!(table_count(&store, "room_vote_states").await, 0);
    assert_eq!(table_count(&store, "room_vote_ballots").await, 0);
    let poll = stored_event(&store, &vote_id).await;
    assert_eq!(poll.extra["message_deleted"], true);
    assert_eq!(poll.extra["vote_question"], "");
    assert_eq!(poll.extra["vote_options"], json!([]));
    let ballot = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE json_extract(event_json, '$.message_kind') = 'vote_cast'",
    )
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load redacted ballot: {error}"));
    let ballot: RoomEvent =
        serde_json::from_str(&ballot).unwrap_or_else(|error| panic!("decode ballot: {error}"));
    assert!(ballot.actor.participant_id.is_empty());
    assert_eq!(ballot.extra["message_deleted"], true);
    assert!(!ballot.extra.contains_key("vote_id"));
    assert!(!ballot.extra.contains_key("vote_choice"));
    assert_rejected(
        store
            .local_room_vote_summary(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &vote_id,
            )
            .await,
        "vote_not_found",
    );
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
    let store = SqliteStore::open("sqlite::memory:")
        .await
        .unwrap_or_else(|error| panic!("open mutation fixture: {error}"));
    store
        .bootstrap_local_authority("71000000-0000-4000-8000-000000000001", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap mutation fixture: {error}"));
    store
        .create_room_for_local_operator(
            "71000000-0000-4000-8000-000000000002",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create mutation room: {error}"));
    let principal = AuthenticatedPrincipal {
        principal_id: LOCAL_OPERATOR_USER_ID.to_owned(),
        participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
        display_name: "Host".to_owned(),
        room_id: "general".to_owned(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    (store, principal)
}

async fn search_count(store: &SqliteStore, query: &str) -> usize {
    store
        .search_local_lobby_messages(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            query,
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("search {query}: {error}"))
        .results
        .len()
}

async fn stored_event(store: &SqliteStore, event_id: &str) -> RoomEvent {
    let encoded = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = 'general' AND json_extract(event_json, '$.id') = ?",
    )
    .bind(event_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load stored event: {error}"));
    serde_json::from_str(&encoded).unwrap_or_else(|error| panic!("decode stored event: {error}"))
}

async fn event_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count events: {error}"))
}

async fn pin_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_message_pins")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count pins: {error}"))
}

async fn stored_attachment_count(store: &SqliteStore, attachment_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_message_attachments WHERE attachment_id = ?")
        .bind(attachment_id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count attachments: {error}"))
}

async fn table_count(store: &SqliteStore, table: &str) -> i64 {
    let query = match table {
        "room_vote_states" => "SELECT COUNT(*) FROM room_vote_states",
        "room_vote_ballots" => "SELECT COUNT(*) FROM room_vote_ballots",
        _ => panic!("unsupported test table"),
    };
    sqlx::query_scalar(query)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count {table}: {error}"))
}

fn assert_rejected<T: std::fmt::Debug>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) if code == expected => {}
        other => panic!("expected {expected} rejection, got {other:?}"),
    }
}
