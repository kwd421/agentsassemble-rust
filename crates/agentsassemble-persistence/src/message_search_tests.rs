use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID,
};
use serde_json::json;

use crate::{PersistenceError, SqliteStore};

#[tokio::test]
async fn complete_search_paginates_and_preserves_unicode_short_and_attachment_matches() {
    let (store, principal) = fixture().await;
    let mut messages = Vec::new();
    for index in 0..65 {
        messages.push(
            send(
                &store,
                &principal,
                &format!("search-page-{index}"),
                &format!("page marker {index}"),
            )
            .await,
        );
    }
    let first = search(&store, "pa ge marker", "").await;
    assert_eq!(first.results.len(), 30);
    assert!(!first.next_cursor.is_empty());
    let second = search(&store, "pa ge marker", &first.next_cursor).await;
    assert_eq!(second.results.len(), 30);
    assert!(!second.next_cursor.is_empty());
    let third = search(&store, "pa ge marker", &second.next_cursor).await;
    assert_eq!(third.results.len(), 5);
    assert!(third.next_cursor.is_empty());
    let found = first
        .results
        .iter()
        .chain(&second.results)
        .chain(&third.results)
        .map(|result| result.event_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(found.len(), 65);
    assert_eq!(
        found.first().copied(),
        messages.last().map(|event| event.id.as_str())
    );
    assert_eq!(
        found.last().copied(),
        messages.first().map(|event| event.id.as_str())
    );

    send(&store, &principal, "search-short-exact", "ab").await;
    send(&store, &principal, "search-short-prefix", "abc").await;
    let short = search(&store, "ab", "").await;
    assert_eq!(short.results.len(), 1);
    assert_eq!(short.results[0].content, "ab");

    send(&store, &principal, "search-casefold", "Straße deployment").await;
    let unicode = search(&store, "STRASSE", "").await;
    assert_eq!(unicode.results.len(), 1);
    assert_eq!(unicode.results[0].content, "Straße deployment");

    let accent = send(&store, &principal, "search-accent", "café release").await;
    assert_eq!(
        search(&store, "cafe", "").await.results[0].event_id,
        accent.id
    );
    let punctuation = send(&store, &principal, "search-punctuation", "deploy-error").await;
    assert_eq!(
        search(&store, "deploy error", "").await.results[0].event_id,
        punctuation.id
    );

    let attachment = store
        .store_message_attachment(
            &principal,
            "evidence.txt",
            "text/plain",
            b"evidence".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store search attachment: {error}"));
    store
        .execute_message(
            &principal,
            "search-attachment",
            "message.send",
            &json!({"content": "", "attachment_ids": [attachment.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("send search attachment: {error}"));
    let attachment_result = search(&store, "evi dence", "").await;
    assert_eq!(attachment_result.results.len(), 1);
    assert_eq!(
        attachment_result.results[0].attachment_filenames,
        ["evidence.txt"]
    );
    assert!(search(&store, ".", "").await.results.is_empty());

    assert_rejection_code(
        store
            .search_local_lobby_messages(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                "page marker",
                "not-a-cursor",
            )
            .await,
        "bad_request",
    );
}

#[tokio::test]
async fn context_is_bounded_chronological_and_uses_public_event_projection() {
    let (store, principal) = fixture().await;
    let mut messages = Vec::new();
    for index in 0..40 {
        messages.push(
            send(
                &store,
                &principal,
                &format!("context-{index}"),
                &format!("context message {index}"),
            )
            .await,
        );
    }
    let target = &messages[20];
    let mut canonical = target.clone();
    canonical.extra.insert(
        "provider_turn_id".to_owned(),
        json!("private-provider-turn"),
    );
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = ? AND seq = ?")
        .bind(
            serde_json::to_string(&canonical)
                .unwrap_or_else(|error| panic!("encode private context event: {error}")),
        )
        .bind(&canonical.room_id)
        .bind(canonical.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("store private context event: {error}"));

    let context = store
        .local_lobby_message_context(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &target.id,
        )
        .await
        .unwrap_or_else(|error| panic!("read message context: {error}"));
    assert_eq!(context.event_id, target.id);
    assert_eq!(context.events.len(), 31);
    assert!(
        context
            .events
            .windows(2)
            .all(|events| events[0].seq < events[1].seq)
    );
    let projected_target = context
        .events
        .iter()
        .find(|event| event.id == target.id)
        .unwrap_or_else(|| panic!("context target missing"));
    assert!(!projected_target.extra.contains_key("provider_turn_id"));
    assert_eq!(
        context.events.first().map(|event| event.id.as_str()),
        Some(messages[5].id.as_str())
    );
    assert_eq!(
        context.events.last().map(|event| event.id.as_str()),
        Some(messages[35].id.as_str())
    );

    assert_rejection_code(
        store
            .local_lobby_message_context(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                "missing-message",
            )
            .await,
        "message_missing",
    );
}

#[tokio::test]
async fn polls_search_by_visible_question_without_indexing_private_transitions() {
    let (store, principal) = fixture().await;
    let before = send(&store, &principal, "vote-search-before", "before poll").await;
    let poll = store
        .execute_message(
            &principal,
            "vote-search-poll",
            "message.send",
            &json!({
                "kind": "vote",
                "vote_question": "Ship privately?",
                "vote_options": ["Yes", "No"]
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("create searchable poll: {error}"))
        .event;
    let transition = store
        .execute_message(
            &principal,
            "vote-search-cast",
            "message.send",
            &json!({"kind": "vote_cast", "vote_id": poll.id, "vote_choice": "Yes"}),
        )
        .await
        .unwrap_or_else(|error| panic!("cast private searchable ballot: {error}"))
        .event;
    let after = send(&store, &principal, "vote-search-after", "after poll").await;

    let results = search(&store, "ship privately", "").await.results;
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].event_id, poll.id);
    assert_eq!(results[0].content, "Ship privately?");

    let indexed = sqlx::query_scalar::<_, String>(
        "SELECT event_id FROM room_message_search_records WHERE room_id = ? ORDER BY event_seq",
    )
    .bind(&principal.room_id)
    .fetch_all(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read vote search index: {error}"));
    assert_eq!(
        indexed,
        [before.id.clone(), poll.id.clone(), after.id.clone()]
    );
    assert!(!indexed.contains(&transition.id));

    let context = store
        .local_lobby_message_context(
            &principal.room_id,
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &poll.id,
        )
        .await
        .unwrap_or_else(|error| panic!("read poll context: {error}"));
    assert_eq!(
        context
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        [before.id.as_str(), poll.id.as_str(), after.id.as_str()]
    );
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
    let store = SqliteStore::open(&format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    ))
    .await
    .unwrap_or_else(|error| panic!("open search store: {error}"));
    store
        .bootstrap_local_authority("a02b41c2-0d9a-49f4-8a18-39de8f933c52", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap search store: {error}"));
    store
        .create_room_for_local_operator(
            "43d0ab4b-8120-48bb-b76c-83478a8d00ef",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create search room: {error}"));
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

async fn send(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    content: &str,
) -> agentsassemble_domain::RoomEvent {
    store
        .execute_message(
            principal,
            request_id,
            "message.send",
            &json!({"content": content}),
        )
        .await
        .unwrap_or_else(|error| panic!("send searchable message: {error}"))
        .event
}

async fn search(store: &SqliteStore, query: &str, cursor: &str) -> crate::LobbyMessageSearchPage {
    store
        .search_local_lobby_messages(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            query,
            cursor,
        )
        .await
        .unwrap_or_else(|error| panic!("search lobby messages: {error}"))
}

fn assert_rejection_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected}, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
}
