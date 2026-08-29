use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, QueuedRoomInput,
};
use serde_json::json;
use sqlx::Row;

use crate::{PersistenceError, RoomCommandMutation};

#[tokio::test]
async fn attachment_only_send_binds_ordered_metadata_and_replays_once() {
    let (store, principal, _directory) = super::fixture().await;
    let first = store
        .store_message_attachment(&principal, "first.txt", "text/plain", b"first".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store first attachment: {error}"));
    let second = store
        .store_message_attachment(
            &principal,
            "second.bin",
            "application/octet-stream",
            b"second".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store second attachment: {error}"));
    let payload = json!({"content": "", "attachment_ids": [second.id, first.id]});

    let committed = store
        .execute_message_with_turn(&principal, "attachment-send", "message.send", &payload)
        .await
        .unwrap_or_else(|error| panic!("send attachment-only message: {error}"));
    let metadata = committed.outcome.event.extra["attachments"]
        .as_array()
        .unwrap_or_else(|| panic!("message event must expose attachment metadata"));
    assert_eq!(
        metadata
            .iter()
            .map(|item| item["id"].as_str().unwrap_or_default())
            .collect::<Vec<_>>(),
        [second.id.as_str(), first.id.as_str()]
    );
    assert_eq!(metadata[0]["filename"], "second.bin");
    assert_eq!(metadata[1]["filename"], "first.txt");
    assert_provider_attachment_assignment(&committed, [&second.id, &first.id]);

    for attachment_id in [&first.id, &second.id] {
        assert_bound_attachment(&store, attachment_id, committed.outcome.event.seq).await;
    }

    let replay = store
        .execute_message_with_turn(&principal, "attachment-send", "message.send", &payload)
        .await
        .unwrap_or_else(|error| panic!("replay attachment send: {error}"));
    assert!(replay.outcome.deduplicated);
    assert_eq!(replay.outcome.event, committed.outcome.event);
    assert!(replay.assignments.is_empty());
    assert!(matches!(
        store
            .execute_message_with_turn(
                &principal,
                "attachment-send",
                "message.send",
                &json!({"content": "changed", "attachment_ids": []}),
            )
            .await,
        Err(PersistenceError::CommandConflict)
    ));

    let third = store
        .store_message_attachment(&principal, "third.txt", "text/plain", b"third".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store third attachment: {error}"));
    let mut foreign_principal = principal.clone();
    foreign_principal.principal_id = "foreign-user".to_owned();
    let before = count_events(&store).await;
    for (request_id, actor, attachment_id) in [
        ("bound-reuse", &principal, first.id.as_str()),
        ("foreign-owner", &foreign_principal, third.id.as_str()),
        (
            "missing-attachment",
            &principal,
            "ma_ffffffffffffffffffffffffffffffff",
        ),
    ] {
        assert_rejected_code(
            store
                .execute_message_with_turn(
                    actor,
                    request_id,
                    "message.send",
                    &json!({"content": "unavailable", "attachment_ids": [attachment_id]}),
                )
                .await,
            "attachment_unavailable",
        );
    }
    assert_eq!(count_events(&store).await, before);
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(&third.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("foreign attachment remains pending: {error}")),
        "pending"
    );
}

fn assert_provider_attachment_assignment(
    committed: &RoomCommandMutation,
    expected_ids: [&String; 2],
) {
    let assignment = committed
        .assignments
        .first()
        .unwrap_or_else(|| panic!("attachment-only message must route to the ordered agent"));
    assert_eq!(
        assignment
            .attachment_ids
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        expected_ids.map(String::as_str)
    );
    for attachment_id in expected_ids {
        assert!(assignment.room_view.contains(attachment_id));
    }
}

#[tokio::test]
async fn expired_attachment_rejects_without_cleanup_or_message() {
    let (store, principal, _directory) = super::fixture().await;
    let attachment = store
        .store_message_attachment(&principal, "expired.txt", "text/plain", b"old".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store expiring attachment: {error}"));
    sqlx::query(
        "UPDATE room_message_attachments SET created_at = 1, expires_at = 2 WHERE attachment_id = ?",
    )
    .bind(&attachment.id)
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("expire attachment: {error}"));
    let before = count_events(&store).await;

    assert_rejected_code(
        store
            .execute_message_with_turn(
                &principal,
                "expired-send",
                "message.send",
                &json!({"content": "must fail", "attachment_ids": [attachment.id]}),
            )
            .await,
        "attachment_unavailable",
    );
    assert_eq!(count_events(&store).await, before);
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT state FROM room_message_attachments WHERE attachment_id = ?",
        )
        .bind(&attachment.id)
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("expired attachment remains owned: {error}")),
        "pending"
    );
}

#[tokio::test]
async fn bound_read_requires_the_canonical_message_reference() {
    let (store, principal, _directory) = super::fixture().await;
    let bytes = b"durable attachment bytes".to_vec();
    let attachment = store
        .store_message_attachment(&principal, "evidence.txt", "text/plain", bytes.clone())
        .await
        .unwrap_or_else(|error| panic!("store readable attachment: {error}"));
    assert_rejected_code(
        store
            .bound_message_attachment(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &attachment.id,
            )
            .await,
        "message_attachment_missing",
    );

    let mut event = store
        .execute_message(
            &principal,
            "readable-attachment",
            "message.send",
            &json!({"content": "evidence", "attachment_ids": [attachment.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("bind readable attachment: {error}"))
        .event;
    let read = store
        .bound_message_attachment(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &attachment.id,
        )
        .await
        .unwrap_or_else(|error| panic!("read bound attachment: {error}"));
    assert_eq!(read.content, bytes);
    assert_eq!(read.metadata.id, attachment.id);
    assert_eq!(read.metadata.filename, "evidence.txt");

    event.extra.remove("attachments");
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = 'general' AND seq = ?")
        .bind(
            serde_json::to_string(&event)
                .unwrap_or_else(|error| panic!("encode unreferenced event: {error}")),
        )
        .bind(event.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("remove canonical reference: {error}"));
    assert_rejected_code(
        store
            .bound_message_attachment(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &attachment.id,
            )
            .await,
        "message_attachment_missing",
    );
}

#[tokio::test]
async fn routing_failure_rolls_back_message_and_attachment_binding() {
    let (store, principal, _directory) = super::fixture().await;
    store
        .execute_message_with_turn(
            &principal,
            "binding-active",
            "message.send",
            &json!({"content": "@Terra hold the floor"}),
        )
        .await
        .unwrap_or_else(|error| panic!("start active turn: {error}"));
    let mut session = super::stored_session(&store).await;
    session.pending_inputs = (0..crate::turn_queue::MAX_QUEUED_EVENT_IDS - 2)
        .map(|index| QueuedRoomInput {
            event_id: format!("binding-queued-{index}"),
            delivery_kind: agentsassemble_domain::RoomInputDeliveryKind::OrderedObservation,
        })
        .collect();
    super::save_stored_session(&store, &session).await;
    store
        .execute_message_with_turn(
            &principal,
            "binding-last-slot",
            "message.send",
            &json!({"content": "@Terra fill the final slot"}),
        )
        .await
        .unwrap_or_else(|error| panic!("fill final queue slot: {error}"));

    let attachment = store
        .store_message_attachment(&principal, "rollback.txt", "text/plain", b"keep".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store rollback attachment: {error}"));
    let before = count_events(&store).await;
    assert_rejected_code(
        store
            .execute_message_with_turn(
                &principal,
                "binding-overflow",
                "message.send",
                &json!({
                    "content": "@Terra this must roll back",
                    "attachment_ids": [attachment.id]
                }),
            )
            .await,
        "ordered_floor_queue_full",
    );
    assert_eq!(count_events(&store).await, before);
    let row = sqlx::query(
        "SELECT pending_owner_user_id, event_seq, state FROM room_message_attachments WHERE attachment_id = ?",
    )
    .bind(&attachment.id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load rolled-back attachment: {error}"));
    assert_eq!(
        row.get::<Option<String>, _>("pending_owner_user_id"),
        Some(principal.principal_id)
    );
    assert_eq!(row.get::<Option<i64>, _>("event_seq"), None);
    assert_eq!(row.get::<String, _>("state"), "pending");
}

async fn count_events(store: &crate::SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_events")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count room events: {error}"))
}

async fn assert_bound_attachment(store: &crate::SqliteStore, attachment_id: &str, event_seq: i64) {
    let row = sqlx::query(
        "SELECT pending_owner_user_id, event_seq, state, expires_at FROM room_message_attachments WHERE attachment_id = ?",
    )
    .bind(attachment_id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("load bound attachment: {error}"));
    assert_eq!(row.get::<Option<String>, _>("pending_owner_user_id"), None);
    assert_eq!(row.get::<Option<i64>, _>("event_seq"), Some(event_seq));
    assert_eq!(row.get::<String, _>("state"), "bound");
    assert_eq!(row.get::<Option<i64>, _>("expires_at"), None);
}

fn assert_rejected_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected}, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
}
