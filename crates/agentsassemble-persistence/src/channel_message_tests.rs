use super::*;
use crate::RoomMutationAuthority::TrustedPrincipal;
use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, RoomSettings, public_settings,
};

fn checked<T, E: std::fmt::Debug>(value: Result<T, E>) -> T {
    value.unwrap_or_else(|error| panic!("channel fixture: {error:?}"))
}

fn channel(id: &str, name: &str, position: u32) -> Value {
    json!({"id":id, "name":name, "type":"text", "position":position, "created_at":"2026-09-08T00:00:00Z"})
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
    let directory = checked(tempfile::tempdir());
    let store = checked(SqliteStore::open_path(&directory.path().join("room.sqlite3")).await);
    checked(
        store
            .bootstrap_local_authority("11111111-1111-4111-8111-111111111111", "Host")
            .await,
    );
    checked(
        store
            .create_room_for_local_operator(
                "22222222-2222-4222-8222-222222222222",
                "general",
                "General",
            )
            .await,
    );
    let principal = AuthenticatedPrincipal {
        principal_id: "operator-local-user".into(),
        participant_id: "operator-local".into(),
        display_name: "Host".into(),
        room_id: "general".into(),
        client_kind: ClientKind::Browser,
        invite_scope: InviteScope::ReadWrite,
        is_operator: true,
        capabilities: CapabilitySet::local_operator(ClientKind::Browser, InviteScope::ReadWrite),
    };
    checked(
        set_channels(
            &store,
            &principal,
            json!([
                channel("c0123456789ab", "First", 0),
                channel("c0123456789ac", "Second", 1)
            ]),
        )
        .await,
    );
    (store, principal, directory)
}

async fn set_channels(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    channels: Value,
) -> Result<CommandOutcome, PersistenceError> {
    let encoded = checked(
        sqlx::query_scalar::<_, String>("SELECT settings_json FROM rooms WHERE room_id = ?")
            .bind(&principal.room_id)
            .fetch_one(&store.pool)
            .await,
    );
    let current: RoomSettings = checked(serde_json::from_str(&encoded));
    let revision = checked(public_settings(&current)).settings_revision;
    store
        .execute_room_settings_update(
            TrustedPrincipal(principal),
            &uuid::Uuid::new_v4().to_string(),
            &json!({"expected_revision":revision, "channels":channels}),
        )
        .await
}

async fn page(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    id: &str,
    before_seq: i64,
    limit: i64,
) -> Result<ChannelHistoryPage, PersistenceError> {
    store
        .channel_history_page(
            TrustedPrincipal(principal),
            id,
            RoomHistoryRequest { before_seq, limit },
        )
        .await
}

#[tokio::test]
async fn channels_have_independent_history_replay_and_restart_without_lobby_turn_inputs() {
    let (store, principal, directory) = fixture().await;
    checked(
        store
            .execute_message(
                &principal,
                "lobby",
                "message.send",
                &json!({"content":"lobby-only"}),
            )
            .await,
    );
    for (request, channel_id, content) in [
        ("one", "c0123456789ab", "first text"),
        ("two", "c0123456789ac", "other channel"),
        ("three", "c0123456789ab", "later text"),
    ] {
        checked(
            store
                .execute_channel_message(
                    TrustedPrincipal(&principal),
                    request,
                    &json!({"channel_id":channel_id,"content":content}),
                )
                .await,
        );
    }
    let repeated = checked(
        store
            .execute_channel_message(
                TrustedPrincipal(&principal),
                "one",
                &json!({"channel_id":"c0123456789ab","content":"first text"}),
            )
            .await,
    );
    assert!(repeated.deduplicated);
    assert!(
        store
            .execute_channel_message(
                TrustedPrincipal(&principal),
                "one",
                &json!({"channel_id":"c0123456789ac","content":"first text"})
            )
            .await
            .is_err()
    );
    let latest = checked(page(&store, &principal, "c0123456789ab", 0, 1).await);
    assert!(latest.has_more_before);
    assert_eq!(latest.events[0].content.as_deref(), Some("later text"));
    let before = checked(page(&store, &principal, "c0123456789ab", latest.oldest_seq, 1).await);
    assert!(!before.has_more_before);
    assert_eq!(before.events[0].id, repeated.event.id);
    assert!(!before.events[0].is_current_lobby_message());
    assert_eq!(
        checked(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM provider_turn_executions")
                .fetch_one(&store.pool)
                .await
        ),
        0
    );
    checked(
        set_channels(
            &store,
            &principal,
            json!([
                channel("c0123456789ac", "Second", 0),
                channel("c0123456789ab", "Renamed", 1)
            ]),
        )
        .await,
    );
    drop(store);
    let reopened = checked(SqliteStore::open_path(&directory.path().join("room.sqlite3")).await);
    let history = checked(page(&reopened, &principal, "c0123456789ab", 0, 80).await);
    assert_eq!(history.events.len(), 2);
    assert_eq!(history.events[0].id, repeated.event.id);
    let search = checked(
        reopened
            .search_local_messages(
                &principal.room_id,
                &principal.principal_id,
                &principal.participant_id,
                "c0123456789ab",
                "first",
                "",
            )
            .await,
    );
    assert_eq!(search.results.len(), 1);
    assert_eq!(search.results[0].event_id, repeated.event.id);
    assert_eq!(search.results[0].channel_id, "c0123456789ab");
    assert!(
        page(&reopened, &principal, "c000000000000", 0, 80)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn channel_retirement_closes_history_replay_and_old_identity_reuse_atomically() {
    let (store, principal, _directory) = fixture().await;
    let payload = json!({"channel_id":"c0123456789ab", "content":"retired content"});
    let message = checked(
        store
            .execute_channel_message(TrustedPrincipal(&principal), "retire", &payload)
            .await,
    );
    checked(
        set_channels(
            &store,
            &principal,
            json!([channel("c0123456789ac", "Second", 0)]),
        )
        .await,
    );
    assert!(
        page(&store, &principal, "c0123456789ab", 0, 80)
            .await
            .is_err()
    );
    assert!(
        store
            .execute_channel_message(TrustedPrincipal(&principal), "retire", &payload)
            .await
            .is_err()
    );
    let history = checked(
        store
            .room_history_page(
                &principal,
                RoomHistoryRequest {
                    before_seq: 0,
                    limit: 80,
                },
            )
            .await,
    );
    let retired = checked(
        history
            .events
            .iter()
            .find(|event| event.id == message.event.id)
            .ok_or("retained sequence"),
    );
    assert_eq!(retired.content.as_deref(), Some(""));
    assert_eq!(retired.extra.get("message_deleted"), Some(&json!(true)));
    let reused = set_channels(
        &store,
        &principal,
        json!([channel("c0123456789ab", "New label", 0)]),
    )
    .await;
    assert!(matches!(
        reused,
        Err(PersistenceError::CommandRejected {
            code: "channel_retired",
            ..
        })
    ));
    let mut changed = channel("c0123456789ac", "Second", 0);
    changed["created_at"] = json!("2026-09-09T00:00:00Z");
    assert!(matches!(
        set_channels(&store, &principal, json!([changed])).await,
        Err(PersistenceError::CommandRejected {
            code: "channel_identity_conflict",
            ..
        })
    ));
}

#[tokio::test]
async fn denied_writes_and_failed_receipt_insert_leave_no_partial_channel_event() {
    let (store, mut principal, _directory) = fixture().await;
    let payload = json!({"channel_id":"c0123456789ab", "content":"blocked"});
    principal.capabilities.message_send = false;
    assert!(matches!(
        store
            .execute_channel_message(TrustedPrincipal(&principal), "denied", &payload)
            .await,
        Err(PersistenceError::CommandRejected {
            code: "permission_denied",
            ..
        })
    ));
    assert!(
        checked(page(&store, &principal, "c0123456789ab", 0, 80).await)
            .events
            .is_empty()
    );
    principal.capabilities.message_send = true;
    checked(sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.muted', json('true')) WHERE room_id = 'general'")
        .execute(&store.pool).await);
    assert!(matches!(
        store
            .execute_channel_message(TrustedPrincipal(&principal), "muted", &payload)
            .await,
        Err(PersistenceError::CommandRejected { code: "muted", .. })
    ));
    checked(sqlx::query("UPDATE participants SET participant_json = json_set(participant_json, '$.muted', json('false')) WHERE room_id = 'general'").execute(&store.pool).await);
    checked(sqlx::query("CREATE TRIGGER reject_channel_receipt BEFORE INSERT ON command_results WHEN NEW.action = 'channel.message.send' BEGIN SELECT RAISE(ABORT, 'fixture rollback'); END").execute(&store.pool).await);
    assert!(
        store
            .execute_channel_message(TrustedPrincipal(&principal), "rollback", &payload)
            .await
            .is_err()
    );
    assert!(
        checked(page(&store, &principal, "c0123456789ab", 0, 80).await)
            .events
            .is_empty()
    );
}

#[tokio::test]
async fn the_same_channel_id_in_two_rooms_never_shares_a_stream() {
    let (store, principal, _directory) = fixture().await;
    checked(
        store
            .create_room_for_local_operator(
                "33333333-3333-4333-8333-333333333333",
                "other",
                "Other",
            )
            .await,
    );
    let mut other = principal.clone();
    other.room_id = "other".into();
    checked(
        set_channels(
            &store,
            &other,
            json!([channel("c0123456789ab", "Same id", 0)]),
        )
        .await,
    );
    for (writer, text) in [(&principal, "general text"), (&other, "other text")] {
        checked(
            store
                .execute_channel_message(
                    TrustedPrincipal(writer),
                    "same-request",
                    &json!({"channel_id":"c0123456789ab", "content":text}),
                )
                .await,
        );
    }
    for (reader, text) in [(&principal, "general text"), (&other, "other text")] {
        let history = checked(page(&store, reader, "c0123456789ab", 0, 80).await);
        assert_eq!(history.events.len(), 1);
        assert_eq!(history.events[0].content.as_deref(), Some(text));
    }
}
