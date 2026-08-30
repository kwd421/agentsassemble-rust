use sqlx::Row;

use crate::schema::tests::installed_schema;

#[tokio::test]
async fn canonical_deletion_removes_record_and_contentless_phrase_row() {
    let pool = installed_schema().await;
    sqlx::query(
        "INSERT INTO rooms(room_id, room_json, settings_json) VALUES ('general', '{}', '{}')",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert room: {error}"));
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES ('general', 1, '{}')")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert event: {error}"));
    sqlx::query("INSERT INTO room_message_search_records(room_id, event_seq, event_id, created_at_nanos, search_text, compact_text) VALUES ('general', 1, 'event-1', 1, 'short token', 'shorttoken')")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert search record: {error}"));
    let row_id = sqlx::query("SELECT id FROM room_message_search_records")
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("read search row: {error}"))
        .get::<i64, _>("id");
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_search_phrase WHERE room_message_search_phrase MATCH '\"short\"'",
        )
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("query phrase row: {error}")),
        1
    );

    sqlx::query("DELETE FROM room_events WHERE room_id = 'general' AND seq = 1")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete canonical event: {error}"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_message_search_records")
            .fetch_one(&pool)
            .await
            .unwrap_or_else(|error| panic!("count records: {error}")),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM room_message_search_phrase WHERE rowid = ?",
        )
        .bind(row_id)
        .fetch_one(&pool)
        .await
        .unwrap_or_else(|error| panic!("count phrase rows: {error}")),
        0
    );
}
