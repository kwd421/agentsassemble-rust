use agentsassemble_domain::{
    LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, QueuedRoomInput,
};
use serde_json::json;
use sqlx::Row;

use crate::{PersistenceError, ProviderAttachmentReadAuthority, RoomCommandMutation};

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
async fn provider_read_revalidates_exact_turn_and_inflight_reference() {
    let (store, principal, _directory) = super::fixture().await;
    let attachment = store
        .store_message_attachment(&principal, "agent.txt", "text/plain", b"agent".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store provider attachment: {error}"));
    let committed = store
        .execute_message_with_turn(
            &principal,
            "provider-attachment",
            "message.send",
            &json!({"content": "@Terra inspect", "attachment_ids": [attachment.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign provider attachment: {error}"));
    let assignment = committed
        .assignments
        .first()
        .unwrap_or_else(|| panic!("provider attachment must assign a turn"));
    store
        .authorize_provider_turn_start(
            &assignment.session.public.room_id,
            &assignment.session.public.session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize provider attachment turn: {error}"));
    let authority = ProviderAttachmentReadAuthority {
        room_id: &assignment.session.public.room_id,
        session_id: &assignment.session.public.session_id,
        turn_id: &assignment.turn_id,
        input_up_to_seq: assignment.session.input_up_to_seq,
        turn_generation: assignment.turn_generation,
        execution_id: &assignment.execution_id,
    };
    let read = store
        .bound_provider_message_attachment(authority, &attachment.id)
        .await
        .unwrap_or_else(|error| panic!("read provider attachment: {error}"));
    assert_eq!(read.metadata.id, attachment.id);
    assert_eq!(read.content, b"agent");

    let pending = store
        .store_message_attachment(&principal, "pending.txt", "text/plain", b"pending".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store unreferenced attachment: {error}"));
    assert_rejected_code(
        store
            .bound_provider_message_attachment(authority, &pending.id)
            .await,
        "message_attachment_missing",
    );
    let stale = ProviderAttachmentReadAuthority {
        input_up_to_seq: authority.input_up_to_seq + 1,
        ..authority
    };
    assert_rejected_code(
        store
            .bound_provider_message_attachment(stale, &attachment.id)
            .await,
        "stale_provider_turn",
    );
}

#[tokio::test]
async fn provider_reads_an_attachment_shown_before_the_message_that_asks_about_it() {
    let (store, principal, _directory) = super::fixture().await;
    let now = chrono::Utc::now();
    let flash = super::participant(
        super::SECOND_AGENT_ID,
        "Flash",
        "agent",
        agentsassemble_domain::ParticipantRole::Agent,
        now,
    );
    let mut flash_session = super::attached_session(now);
    super::SECOND_AGENT_ID.clone_into(&mut flash_session.public.session_id);
    super::SECOND_AGENT_ID.clone_into(&mut flash_session.public.participant_id);
    "Flash".clone_into(&mut flash_session.public.display_name);
    "provider-thread-2".clone_into(&mut flash_session.provider_session_id);
    "owned-runtime-2".clone_into(&mut flash_session.runtime_handle_id);
    super::insert_agent(&store, &flash, &flash_session).await;
    let attachment = store
        .store_message_attachment(&principal, "photo.txt", "text/plain", b"photo".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store earlier attachment: {error}"));
    // The attachment goes to another agent's turn; Terra only sees it in its room view.
    let for_flash = store
        .execute_message_with_turn(
            &principal,
            "attachment-for-flash",
            "message.send",
            &json!({"content": "@Flash what is this?", "attachment_ids": [attachment.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("send attachment message: {error}"));
    let flash_turn = for_flash
        .assignments
        .first()
        .unwrap_or_else(|| panic!("the attachment message must assign Flash"));
    assert_eq!(flash_turn.session.public.session_id, super::SECOND_AGENT_ID);
    let asked = store
        .execute_message_with_turn(
            &principal,
            "ask-terra",
            "message.send",
            &json!({"content": "@Terra can you see that file?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("ask Terra: {error}"));
    let flash_start = super::running_authority(&store, flash_turn, "provider-turn-flash").await;
    let finished = store
        .complete_agent_turn(
            "general",
            super::SECOND_AGENT_ID,
            super::authority(&flash_start, "provider-turn-flash", None),
            "A text file.",
            "",
        )
        .await
        .unwrap_or_else(|error| panic!("finish Flash turn: {error}"));
    let assignment = asked
        .assignments
        .iter()
        .chain(finished.next_assignments.iter())
        .find(|assignment| assignment.session.public.session_id == super::AGENT_ID)
        .unwrap_or_else(|| panic!("the question must assign Terra"));
    assert!(assignment.room_view.contains(&attachment.id));
    assert_eq!(assignment.attachment_ids, [attachment.id.clone()]);

    store
        .authorize_provider_turn_start(
            &assignment.session.public.room_id,
            &assignment.session.public.session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize Terra turn: {error}"));
    let read = store
        .bound_provider_message_attachment(
            ProviderAttachmentReadAuthority {
                room_id: &assignment.session.public.room_id,
                session_id: &assignment.session.public.session_id,
                turn_id: &assignment.turn_id,
                input_up_to_seq: assignment.session.input_up_to_seq,
                turn_generation: assignment.turn_generation,
                execution_id: &assignment.execution_id,
            },
            &attachment.id,
        )
        .await
        .unwrap_or_else(|error| panic!("read the earlier attachment: {error}"));
    assert_eq!(read.content, b"photo");
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

#[tokio::test]
async fn queued_attachment_messages_keep_whole_events_within_turn_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let (store, principal, _directory) = super::fixture().await;
    let active = store
        .execute_message_with_turn(
            &principal,
            "hold-attachments",
            "message.send",
            &json!({"content":"@Terra hold"}),
        )
        .await?;
    let active = &active.assignments[0];
    let mut batches = Vec::new();
    for (index, count) in [5, 4].into_iter().enumerate() {
        let mut ids = Vec::new();
        for item in 0..count {
            let attachment = store
                .store_message_attachment(
                    &principal,
                    &format!("{index}-{item}.txt"),
                    "text/plain",
                    vec![b'a'],
                )
                .await?;
            ids.push(attachment.id);
        }
        let queued = store
            .execute_message_with_turn(
                &principal,
                &format!("queued-attachments-{index}"),
                "message.send",
                &json!({"content":"@Terra inspect attachments", "attachment_ids":ids}),
            )
            .await?;
        assert!(queued.assignments.is_empty());
        batches.push((queued.outcome.event.id, ids));
    }
    let started = super::running_authority(&store, active, "held-native").await;
    let committed = store
        .complete_agent_turn(
            "general",
            super::AGENT_ID,
            super::authority(&started, "held-native", None),
            "held complete",
            "",
        )
        .await?;
    let first = &committed.next_assignments[0];
    assert_eq!(first.attachment_ids, batches[0].1);
    assert_eq!(
        super::input_ids(&first.session.pending_inputs),
        [batches[1].0.clone()]
    );
    assert_eq!(first.session.input_up_to_event_id, batches[0].0);
    let started = super::running_authority(&store, first, "first-attachments").await;
    let committed = store
        .complete_agent_turn(
            "general",
            super::AGENT_ID,
            super::authority(&started, "first-attachments", None),
            "first complete",
            "",
        )
        .await?;
    let second = &committed.next_assignments[0];
    assert_eq!(second.attachment_ids, batches[1].1);
    assert!(second.session.pending_inputs.is_empty());
    assert_eq!(second.session.input_up_to_event_id, batches[1].0);
    Ok(())
}
