use serde_json::json;

use agentsassemble_domain::AuthenticatedPrincipal;

use crate::{PersistenceError, ProviderMessageSearchAuthority, SqliteStore};

#[tokio::test]
async fn provider_search_revalidates_the_exact_active_turn_without_writing() {
    let (store, principal, _directory) = super::fixture().await;
    let committed = store
        .execute_message_with_turn(
            &principal,
            "provider-search-source",
            "message.send",
            &json!({"content": "search exact marker ALPHA-0830"}),
        )
        .await
        .unwrap_or_else(|error| panic!("assign provider search turn: {error}"));
    let assignment = committed
        .assignments
        .first()
        .unwrap_or_else(|| panic!("search message must assign a provider turn"));
    store
        .authorize_provider_turn_start(
            &assignment.session.public.room_id,
            &assignment.session.public.session_id,
            assignment.turn_generation,
            &assignment.turn_id,
        )
        .await
        .unwrap_or_else(|error| panic!("authorize provider search turn: {error}"));
    let authority = ProviderMessageSearchAuthority {
        room_id: &assignment.session.public.room_id,
        session_id: &assignment.session.public.session_id,
        turn_id: &assignment.turn_id,
        input_up_to_seq: assignment.session.input_up_to_seq,
        turn_generation: assignment.turn_generation,
        execution_id: &assignment.execution_id,
    };
    let (poll_id, transition_id) = create_poll_with_private_ballot(&store, &principal).await;
    let events_before =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events WHERE room_id = 'general'")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count events before provider search: {error}"));

    let page = store
        .search_provider_lobby_messages(authority, "ALPHA-0830", "")
        .await
        .unwrap_or_else(|error| panic!("search as provider turn: {error}"));
    assert_eq!(page.results.len(), 1);
    assert_eq!(page.results[0].event_id, committed.outcome.event.id);
    let context = store
        .provider_lobby_message_context(authority, &page.results[0].event_id)
        .await
        .unwrap_or_else(|error| panic!("read provider message context: {error}"));
    assert_eq!(context.event_id, committed.outcome.event.id);
    assert_eq!(
        context
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        [committed.outcome.event.id.as_str(), poll_id.as_str()]
    );
    let authored = store
        .search_provider_lobby_messages(authority, "Host", "")
        .await
        .unwrap_or_else(|error| panic!("search provider-visible author: {error}"));
    assert!(
        authored
            .results
            .iter()
            .any(|result| result.event_id == poll_id)
    );
    assert!(
        authored
            .results
            .iter()
            .all(|result| result.event_id != transition_id)
    );
    let events_after =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_events WHERE room_id = 'general'")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count events after provider search: {error}"));
    assert_eq!(events_after, events_before);

    let stale = ProviderMessageSearchAuthority {
        input_up_to_seq: authority.input_up_to_seq + 1,
        ..authority
    };
    assert_rejection_code(
        store
            .search_provider_lobby_messages(stale, "ALPHA-0830", "")
            .await,
        "stale_provider_turn",
    );
}

async fn create_poll_with_private_ballot(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
) -> (String, String) {
    let poll = store
        .execute_message_with_turn(
            principal,
            "provider-search-poll",
            "message.send",
            &json!({
                "kind": "vote",
                "vote_question": "Provider-visible question?",
                "vote_options": ["A", "B"]
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("create provider-search poll: {error}"));
    assert!(poll.assignments.is_empty());
    let transition = store
        .execute_message_with_turn(
            principal,
            "provider-search-ballot",
            "message.send",
            &json!({
                "kind": "vote_cast",
                "vote_id": poll.outcome.event.id,
                "vote_choice": "B"
            }),
        )
        .await
        .unwrap_or_else(|error| panic!("cast provider-search ballot: {error}"));
    assert!(transition.assignments.is_empty());
    (poll.outcome.event.id, transition.outcome.event.id)
}

fn assert_rejection_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected}, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
}
