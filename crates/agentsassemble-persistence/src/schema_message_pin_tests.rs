use super::tests::installed_schema;

async fn seed_room_event(pool: &sqlx::SqlitePool, room_id: &str, seq: i64, event_id: &str) {
    sqlx::query("INSERT INTO rooms(room_id, room_json, settings_json) VALUES (?, '{}', '{}')")
        .bind(room_id)
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert room: {error}"));
    sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
        .bind(room_id)
        .bind(seq)
        .bind(format!(r#"{{"id":"{event_id}"}}"#))
        .execute(pool)
        .await
        .unwrap_or_else(|error| panic!("insert room event: {error}"));
}

#[tokio::test]
async fn message_pin_schema_owns_bounded_event_pointers() {
    let pool = installed_schema().await;
    seed_room_event(&pool, "general", 1, "event-1").await;

    sqlx::query(
        "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES ('general', 'event-1', 1, 1)",
    )
    .execute(&pool)
    .await
    .unwrap_or_else(|error| panic!("insert valid pin: {error}"));

    for invalid_event_id in ["", &"x".repeat(129), "event\0tail"] {
        assert!(
            sqlx::query(
                "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES ('general', ?, 1, 1)",
            )
            .bind(invalid_event_id)
            .execute(&pool)
            .await
            .is_err(),
            "accepted invalid event ID"
        );
    }
    assert!(
        sqlx::query(
            "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES ('general', 'event-2', 2, 1)",
        )
        .execute(&pool)
        .await
        .is_err(),
        "accepted a pointer without an owned room event"
    );
    assert!(
        sqlx::query(
            "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES ('general', 'event-2', 1, 1)",
        )
        .execute(&pool)
        .await
        .is_err(),
        "accepted a second identity for one event sequence"
    );
}

#[tokio::test]
async fn deleting_the_owned_event_or_room_removes_only_its_pin() {
    let pool = installed_schema().await;
    seed_room_event(&pool, "room-a", 1, "event-a").await;
    seed_room_event(&pool, "room-b", 1, "event-b").await;
    for (room_id, event_id) in [("room-a", "event-a"), ("room-b", "event-b")] {
        sqlx::query(
            "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES (?, ?, 1, 1)",
        )
        .bind(room_id)
        .bind(event_id)
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("insert pin: {error}"));
    }

    sqlx::query("DELETE FROM room_events WHERE room_id = 'room-a' AND seq = 1")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete owned event: {error}"));
    assert_eq!(pin_count(&pool, "room-a").await, 0);
    assert_eq!(pin_count(&pool, "room-b").await, 1);

    sqlx::query("DELETE FROM rooms WHERE room_id = 'room-b'")
        .execute(&pool)
        .await
        .unwrap_or_else(|error| panic!("delete owning room: {error}"));
    assert_eq!(pin_count(&pool, "room-b").await, 0);
}

async fn pin_count(pool: &sqlx::SqlitePool, room_id: &str) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_message_pins WHERE room_id = ?")
        .bind(room_id)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| panic!("count pins: {error}"))
}
