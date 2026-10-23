use serde_json::json;

use super::{AGENT_ID, authority, fixture, running_authority, save_stored_session, stored_session};

#[tokio::test]
async fn api_turns_preserve_creation_context_after_store_reopen() {
    let (store, principal, directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.provider_kind = "deepseek_api".to_owned();
    session.public.runtime_kind = "api".to_owned();
    session.public.transport = "https".to_owned();
    session.public.provider_session_reused = false;
    save_stored_session(&store, &session).await;

    let first = store
        .execute_message_with_turn(
            &principal,
            "api-context-first",
            "message.send",
            &json!({"content": "@Terra archive project code ORCHID-71"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign first API turn: {error}"));
    let first_assignment = first
        .assignments
        .first()
        .unwrap_or_else(|| panic!("first API message must be assigned"));
    let first_start = running_authority(&store, first_assignment, "api-provider-turn-1").await;
    store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&first_start, "api-provider-turn-1", None),
            crate::AgentFinalMessage {
                content: "Understood.",
                target_agent_id: "",
                reply_to_event_id: None,
                attachment_ids: &[],
            },
        )
        .await
        .unwrap_or_else(|error| panic!("complete first API turn: {error}"));
    assert_eq!(stored_session(&store).await.public.bootstrap_cutoff_seq, 0);

    drop(store);
    let store = super::SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
        .await
        .unwrap_or_else(|error| panic!("reopen API context store: {error}"));

    let second = store
        .execute_message_with_turn(
            &principal,
            "api-context-second",
            "message.send",
            &json!({"content": "@Terra which project code did I give you?"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign second API turn: {error}"));
    let assignment = second
        .assignments
        .first()
        .unwrap_or_else(|| panic!("second API message must be assigned"));
    let room_view = &assignment.room_view;

    assert!(room_view.contains("archive project code ORCHID-71"));
    assert!(room_view.contains("Understood."));
    assert!(room_view.contains("which project code did I give you?"));
    // A harness that still holds the first turn reads only what followed it,
    // without its own reply.
    let delta = assignment
        .room_view_delta
        .as_ref()
        .unwrap_or_else(|| panic!("a completed API turn is continued"));
    assert_eq!(delta.since_seq, first_assignment.session.input_up_to_seq);
    assert!(delta.view.contains("which project code did I give you?"));
    assert!(!delta.view.contains("ORCHID-71"));
    assert!(!delta.view.contains("Understood."));
}

#[tokio::test]
async fn deleted_vote_does_not_block_the_next_api_message() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.provider_kind = "deepseek_api".to_owned();
    session.public.runtime_kind = "api".to_owned();
    session.public.transport = "https".to_owned();
    session.public.provider_session_reused = false;
    save_stored_session(&store, &session).await;
    let poll = store.execute_message_with_turn(
        &principal, "context-poll", "message.send",
        &json!({"kind": "vote", "vote_question": "deleted-secret-question", "vote_options": ["Yes", "No"]}),
    ).await.unwrap_or_else(|error| panic!("create poll: {error}"));
    let assignment = poll
        .assignments
        .first()
        .unwrap_or_else(|| panic!("poll assigns API agent"));
    let start = running_authority(&store, assignment, "context-poll-turn").await;
    store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "context-poll-turn", None),
            crate::AgentFinalMessage {
                content: "Poll received.",
                target_agent_id: "",
                reply_to_event_id: None,
                attachment_ids: &[],
            },
        )
        .await
        .unwrap_or_else(|_| panic!("finish poll turn"));
    store
        .execute_message_mutation(
            &principal,
            "context-delete",
            "message.delete",
            &json!({"event_id": poll.outcome.event.id}),
        )
        .await
        .unwrap_or_else(|_| panic!("delete poll"));
    let next = store
        .execute_message_with_turn(
            &principal,
            "context-after-delete",
            "message.send",
            &json!({"content": "@Terra continue after deletion"}),
        )
        .await
        .unwrap_or_else(|_| panic!("commit and assign next message"));
    let view = &next
        .assignments
        .first()
        .unwrap_or_else(|| panic!("next assignment"))
        .room_view;
    assert!(view.contains("continue after deletion"));
    assert!(!view.contains("deleted-secret-question"));
    assert!(!view.contains(&poll.outcome.event.id));
    let committed: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM room_events WHERE json_extract(event_json, '$.id') = ?",
    )
    .bind(&next.outcome.event.id)
    .fetch_one(&store.pool)
    .await
    .unwrap_or_else(|_| panic!("read committed message"));
    assert_eq!(committed, 1);
}

#[tokio::test]
async fn a_deleted_message_reopens_the_api_conversation_from_current_rows() {
    let (store, principal, _directory) = fixture().await;
    let mut session = stored_session(&store).await;
    session.public.provider_kind = "deepseek_api".to_owned();
    session.public.runtime_kind = "api".to_owned();
    session.public.transport = "https".to_owned();
    save_stored_session(&store, &session).await;
    let first = store
        .execute_message_with_turn(
            &principal,
            "carried-secret",
            "message.send",
            &json!({"content": "@Terra the door code is 4417"}),
        )
        .await
        .unwrap();
    let start = running_authority(&store, &first.assignments[0], "carried-turn-1").await;
    store
        .complete_agent_turn(
            "general",
            AGENT_ID,
            authority(&start, "carried-turn-1", None),
            crate::AgentFinalMessage {
                content: "Noted.",
                target_agent_id: "",
                reply_to_event_id: None,
                attachment_ids: &[],
            },
        )
        .await
        .unwrap();
    store
        .execute_message_mutation(
            &principal,
            "carried-delete",
            "message.delete",
            &json!({"event_id": first.outcome.event.id}),
        )
        .await
        .unwrap();
    let second = store
        .execute_message_with_turn(
            &principal,
            "carried-next",
            "message.send",
            &json!({"content": "@Terra anything new?"}),
        )
        .await
        .unwrap();
    let assignment = &second.assignments[0];
    // The deleted message may sit in the carried conversation, so the turn
    // reopens from the current window, which no longer has it.
    assert!(assignment.room_view_delta.is_none());
    assert!(!assignment.room_view.contains("4417"));
}
