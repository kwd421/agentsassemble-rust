use super::{ORIGIN, fixture};
use crate::{OwnerDeviceDescription, RoomManagerAuthority, RoomSessionAuthorization, SqliteStore};
use chrono::{Duration, Utc};
use serde_json::json;

type TestResult = Result<(), Box<dyn std::error::Error>>;

async fn paired() -> (SqliteStore, RoomSessionAuthorization) {
    let (store, manager) = fixture("sqlite::memory:").await;
    let now = Utc::now() - Duration::seconds(120);
    store
        .create_operator_pairing(
            &RoomManagerAuthority::Local(manager),
            &[71; 32],
            ORIGIN,
            now,
        )
        .await
        .unwrap_or_else(|e| panic!("grant: {e}"));
    let paired = store
        .redeem_operator_pairing(&[71; 32], &[72; 32], ORIGIN, now)
        .await
        .unwrap_or_else(|e| panic!("redeem: {e}"));
    (
        store,
        RoomSessionAuthorization::Operator(paired.authorization),
    )
}

async fn last_use(store: &SqliteStore) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT last_connected_at FROM operator_pairings")
        .fetch_one(&store.pool)
        .await
}

async fn age(store: &SqliteStore) -> Result<i64, sqlx::Error> {
    let old = (Utc::now() - Duration::days(30) + Duration::seconds(120)).timestamp();
    sqlx::query("UPDATE operator_pairings SET last_connected_at = ?")
        .bind(old)
        .execute(&store.pool)
        .await?;
    Ok(old)
}

#[tokio::test]
async fn activity_command_and_retry_commit_before_any_socket_delivery() -> TestResult {
    let (store, session) = paired().await;
    let sent = store
        .execute_authorized_message_with_turn(
            session.mutation_authority(),
            "send",
            "message.send",
            &json!({"content":"before"}),
        )
        .await?;
    let payload = json!({"event_id": sent.outcome.event.id, "content":"after"});
    for retry in [false, true] {
        let old = age(&store).await?;
        let result = store
            .execute_room_session_message_mutation(&session, "edit", "message.edit", &payload)
            .await?;
        assert_eq!(result.deduplicated, retry);
        assert!(
            last_use(&store).await? > old,
            "successful edit/retry must refresh before returning"
        );
    }
    Ok(())
}

#[tokio::test]
async fn activity_write_failure_rolls_back_message_and_receipt() -> TestResult {
    let (store, session) = paired().await;
    let sent = store
        .execute_authorized_message_with_turn(
            session.mutation_authority(),
            "send",
            "message.send",
            &json!({"content":"before"}),
        )
        .await?;
    let old = age(&store).await?;
    sqlx::query("CREATE TRIGGER fail_activity BEFORE UPDATE OF last_connected_at ON operator_pairings BEGIN SELECT RAISE(ABORT, 'activity write failed'); END")
        .execute(&store.pool).await?;
    let payload = json!({"event_id":sent.outcome.event.id,"content":"after"});
    assert!(
        store
            .execute_room_session_message_mutation(&session, "edit", "message.edit", &payload)
            .await
            .is_err(),
        "activity failure must prevent command success"
    );
    assert_eq!(last_use(&store).await?, old);
    let snapshot = store.snapshot("general", 0, 100).await?;
    let original = snapshot
        .events
        .iter()
        .find(|event| event.id == sent.outcome.event.id)
        .ok_or("message missing")?;
    assert_eq!(original.content.as_deref(), Some("before"));
    sqlx::query("DROP TRIGGER fail_activity")
        .execute(&store.pool)
        .await?;
    let result = store
        .execute_room_session_message_mutation(&session, "edit", "message.edit", &payload)
        .await?;
    assert!(
        !result.deduplicated,
        "failed transaction must not retain a receipt"
    );
    assert!(last_use(&store).await? > old);
    Ok(())
}

#[tokio::test]
async fn activity_identical_ticket_metadata_affects_zero_rows() -> TestResult {
    let (store, session) = paired().await;
    let RoomSessionAuthorization::Operator(operator) = session else {
        panic!("operator")
    };
    // Count real UPDATE effects, including writes whose values do not change.
    sqlx::query("CREATE TABLE activity_updates (id INTEGER)")
        .execute(&store.pool)
        .await?;
    sqlx::query("CREATE TRIGGER count_activity AFTER UPDATE ON operator_pairings BEGIN INSERT INTO activity_updates VALUES (1); END")
        .execute(&store.pool).await?;
    let mut description = OwnerDeviceDescription {
        device_name: "Browser".into(),
        browser: "Firefox".into(),
        os: "Linux".into(),
    };
    store
        .record_operator_connection(&operator, Some(&description))
        .await?;
    let first = last_use(&store).await?;
    sqlx::query("DELETE FROM activity_updates")
        .execute(&store.pool)
        .await?;
    store
        .record_operator_connection(&operator, Some(&description))
        .await?;
    let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_updates")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(
        writes, 0,
        "identical second ticket exchange must affect zero rows"
    );
    assert_eq!(last_use(&store).await?, first);
    description.device_name = "Renamed".into();
    store
        .record_operator_connection(&operator, Some(&description))
        .await?;
    let writes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_updates")
        .fetch_one(&store.pool)
        .await?;
    assert_eq!(writes, 1, "changed metadata must persist within the minute");
    assert_eq!(last_use(&store).await?, first);
    Ok(())
}

#[tokio::test]
async fn activity_room_reads_commands_and_retries_refresh_in_their_owner() -> TestResult {
    use agentsassemble_domain::public_settings;
    let (store, session) = paired().await;
    let revision =
        public_settings(&store.snapshot("general", 0, 1).await?.settings)?.settings_revision;
    let settings = json!({"expected_revision":revision,"channels":[{"id":"c0123456789ab","name":"Chat","type":"text","position":0,"created_at":"2026-09-08T00:00:00Z"}]});
    let message = json!({"content":"message"});
    let channel = json!({"channel_id":"c0123456789ab","content":"channel message"});
    let vote = json!({"kind":"vote","vote_question":"Proceed?","vote_options":["Go","Wait"],"vote_duration_seconds":300});
    for retry in [false, true] {
        for (id, payload) in [
            ("settings", &settings),
            ("message", &message),
            ("channel", &channel),
            ("vote", &vote),
        ] {
            let old = age(&store).await?;
            let authority = session.mutation_authority();
            let result = match id {
                "settings" => {
                    store
                        .execute_room_settings_update(authority, id, payload)
                        .await?
                }
                "channel" => {
                    store
                        .execute_channel_message(authority, id, payload)
                        .await?
                }
                _ => {
                    store
                        .execute_authorized_message_with_turn(
                            authority,
                            id,
                            "message.send",
                            payload,
                        )
                        .await?
                        .outcome
                }
            };
            assert_eq!(result.deduplicated, retry, "{id}");
            assert!(last_use(&store).await? > old, "{id} retry={retry}");
        }
    }
    let vote = store
        .execute_authorized_message_with_turn(
            session.mutation_authority(),
            "vote",
            "message.send",
            &vote,
        )
        .await?
        .outcome
        .event
        .id;
    assert_activity_reads(&store, &session, &vote).await
}

async fn assert_activity_reads(
    store: &SqliteStore,
    session: &RoomSessionAuthorization,
    vote: &str,
) -> TestResult {
    use agentsassemble_domain::RoomHistoryRequest;
    for read in ["room", "channel", "vote", "status"] {
        let old = age(store).await?;
        let authority = session.mutation_authority();
        let request = RoomHistoryRequest {
            before_seq: 0,
            limit: 50,
        };
        match read {
            "room" => {
                store.room_history_page(authority, request).await?;
            }
            "channel" => {
                store
                    .channel_history_page(authority, "c0123456789ab", request)
                    .await?;
            }
            "vote" => {
                store.authorized_room_vote_summary(authority, vote).await?;
            }
            _ => {
                store.conversation_status(authority, 0).await?;
            }
        }
        assert!(last_use(store).await? > old, "{read} read");
    }
    let old = age(store).await?;
    assert!(
        store
            .room_history_page(
                session.mutation_authority(),
                RoomHistoryRequest {
                    before_seq: -1,
                    limit: 50
                }
            )
            .await
            .is_err()
    );
    assert!(
        store
            .execute_authorized_message_with_turn(
                session.mutation_authority(),
                "invalid",
                "message.send",
                &json!({"content":""})
            )
            .await
            .is_err()
    );
    assert_eq!(last_use(store).await?, old, "rejections remain read-only");
    Ok(())
}

#[tokio::test]
async fn activity_side_chat_failure_never_publishes_and_retry_refreshes() -> TestResult {
    let (store, session) = paired().await;
    let now = Utc::now();
    let snapshot = store
        .side_chat_snapshot(session.mutation_authority(), now)
        .await?;
    let mut live = store
        .subscribe_side_chat(session.mutation_authority(), snapshot.room_uid)
        .await?;
    let payload = json!({"generation":snapshot.generation,"after_seq":0,"content":"private"});
    let old = age(&store).await?;
    sqlx::query("CREATE TRIGGER fail_activity BEFORE UPDATE OF last_connected_at ON operator_pairings BEGIN SELECT RAISE(ABORT, 'activity write failed'); END").execute(&store.pool).await?;
    assert!(
        store
            .execute_side_chat(session.mutation_authority(), "side", &payload, now)
            .await
            .is_err()
    );
    assert_eq!(last_use(&store).await?, old);
    assert!(matches!(
        live.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    sqlx::query("DROP TRIGGER fail_activity")
        .execute(&store.pool)
        .await?;
    assert!(
        store
            .side_chat_snapshot(session.mutation_authority(), now)
            .await?
            .messages
            .is_empty()
    );
    for retry in [false, true] {
        let old = age(&store).await?;
        let result = store
            .execute_side_chat(session.mutation_authority(), "side", &payload, now)
            .await?;
        assert_eq!(result.deduplicated, retry);
        assert!(last_use(&store).await? > old);
    }
    assert_eq!(live.try_recv()?.message.content, "private");
    assert!(matches!(
        live.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    Ok(())
}
