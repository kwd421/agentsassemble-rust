use serde_json::json;

use crate::{AgentTurnCommit, RoomCommandMutation, SqliteStore};

pub(super) async fn assert_projection(
    store: &SqliteStore,
    first: &RoomCommandMutation,
    second: &RoomCommandMutation,
    committed: &AgentTurnCommit,
) {
    let mut transaction = store
        .pool
        .begin()
        .await
        .unwrap_or_else(|error| panic!("begin private-message check: {error}"));
    let mut excluded = first.outcome.event.clone();
    excluded.id = "owner-only-search-event".to_owned();
    excluded.seq = crate::room_event_sequence::next_sequence(&mut transaction, "general")
        .await
        .unwrap_or_else(|error| panic!("sequence owner-only message: {error}"));
    excluded
        .extra
        .insert("visibility".to_owned(), json!("owner"));
    super::super::support::insert_event(&mut transaction, &excluded)
        .await
        .unwrap_or_else(|error| panic!("insert owner-only message: {error}"));
    excluded.id = "deleted-search-event".to_owned();
    excluded.seq = crate::room_event_sequence::next_sequence(&mut transaction, "general")
        .await
        .unwrap_or_else(|error| panic!("sequence deleted message: {error}"));
    excluded.extra.remove("visibility");
    excluded
        .extra
        .insert("message_deleted".to_owned(), json!(true));
    super::super::support::insert_event(&mut transaction, &excluded)
        .await
        .unwrap_or_else(|error| panic!("insert deleted message: {error}"));
    transaction
        .commit()
        .await
        .unwrap_or_else(|error| panic!("commit excluded messages: {error}"));
    let records = sqlx::query_as::<_, (String, String)>(
        "SELECT event_id, search_text FROM room_message_search_records \
         WHERE room_id = 'general' ORDER BY event_seq",
    )
    .fetch_all(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("read message search projection: {error}"));
    assert_eq!(
        records,
        [
            (
                first.outcome.event.id.clone(),
                "host\n@terra take the first turn".to_owned(),
            ),
            (
                second.outcome.event.id.clone(),
                "host\n@terra queue this while busy".to_owned(),
            ),
            (
                committed.events[0].id.clone(),
                "terra\nfirst provider final".to_owned(),
            ),
        ]
    );
}
