use agentsassemble_domain::{
    AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope, LOCAL_OPERATOR_PARTICIPANT_ID,
    LOCAL_OPERATOR_USER_ID, MAX_LOBBY_MESSAGE_PINS,
};
use chrono::{Duration, Utc};
use serde_json::json;

use crate::{
    HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence, PersistenceError,
    PreparedHumanAdmission, SqliteStore,
};

const SIGNED: [u8; 32] = [0x61; 32];
const JOIN: [u8; 32] = [0x62; 32];
const BROWSER: [u8; 32] = [0x63; 32];

#[tokio::test]
async fn local_pin_lifecycle_projects_only_canonical_messages() {
    let (store, principal) = fixture().await;
    let first = send(&store, &principal, "message-1", "first").await;
    let second = send(&store, &principal, "message-2", "second").await;

    store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &first.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin first: {error}"));
    sqlx::query("UPDATE room_message_pins SET pinned_at = 1 WHERE event_id = ?")
        .bind(&first.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("age first pin: {error}"));
    let pins = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &second.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin second: {error}"));
    assert_eq!(
        pins.iter()
            .map(|pin| pin.content.as_str())
            .collect::<Vec<_>>(),
        ["second", "first"]
    );
    assert_eq!(pins[0].author, "Host");
    assert!(pins.iter().all(|pin| pin.attachment_filenames.is_empty()));

    sqlx::query("UPDATE room_message_pins SET pinned_at = 2 WHERE event_id = ?")
        .bind(&second.id)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("bound second pin timestamp: {error}"));
    let repinned = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &first.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("re-pin first: {error}"));
    assert_eq!(repinned.len(), 2);
    assert_eq!(repinned[0].event_id, first.id);
    let remaining = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &second.id,
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("unpin second: {error}"));
    assert_eq!(remaining.len(), 1);
    let unchanged = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &second.id,
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("repeat unpin: {error}"));
    assert_eq!(unchanged, remaining);
}

#[tokio::test]
async fn attachment_only_pin_projects_canonical_filenames() {
    let (store, principal) = fixture().await;
    let first = store
        .store_message_attachment(&principal, "first.txt", "text/plain", b"first".to_vec())
        .await
        .unwrap_or_else(|error| panic!("store first pin attachment: {error}"));
    let second = store
        .store_message_attachment(
            &principal,
            "second.bin",
            "application/octet-stream",
            b"second".to_vec(),
        )
        .await
        .unwrap_or_else(|error| panic!("store second pin attachment: {error}"));
    let event = store
        .execute_message(
            &principal,
            "attachment-pin-target",
            "message.send",
            &json!({"content": "", "attachment_ids": [second.id, first.id]}),
        )
        .await
        .unwrap_or_else(|error| panic!("send attachment-only pin target: {error}"))
        .event;

    let pins = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &event.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin attachment-only message: {error}"));
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].content, "");
    assert_eq!(pins[0].attachment_filenames, ["second.bin", "first.txt"]);

    let mut corrupt = event;
    corrupt
        .extra
        .get_mut("attachments")
        .unwrap_or_else(|| panic!("attachment metadata missing"))[0]["url"] = json!("/wrong");
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = 'general' AND seq = ?")
        .bind(
            serde_json::to_string(&corrupt)
                .unwrap_or_else(|error| panic!("encode corrupt attachment event: {error}")),
        )
        .bind(corrupt.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("store corrupt attachment event: {error}"));
    assert_rejection_code(
        store
            .local_lobby_message_pins(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await,
        "invalid_state",
    );
}

#[tokio::test]
async fn missing_nonmessage_and_invalid_targets_leave_no_pin() {
    let (store, principal) = fixture().await;
    let room_created = store
        .snapshot("general", 0, 100)
        .await
        .unwrap_or_else(|error| panic!("snapshot: {error}"))
        .events
        .into_iter()
        .find(|event| event.event_type == "room_created")
        .unwrap_or_else(|| panic!("room-created event missing"));

    for pinned in [true, false] {
        for event_id in ["missing", room_created.id.as_str(), "bad\0id"] {
            assert!(
                store
                    .set_local_lobby_message_pin(
                        "general",
                        LOCAL_OPERATOR_USER_ID,
                        LOCAL_OPERATOR_PARTICIPANT_ID,
                        event_id,
                        pinned,
                    )
                    .await
                    .is_err()
            );
        }
    }
    let valid = send(&store, &principal, "message-valid", "valid").await;
    let before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_message_pins")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count pins: {error}"));
    assert_eq!(before, 0);

    store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &valid.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("pin valid message: {error}"));
    let mut missing_content = valid.clone();
    missing_content.content = None;
    sqlx::query("UPDATE room_events SET event_json = ? WHERE room_id = 'general' AND seq = ?")
        .bind(
            serde_json::to_string(&missing_content)
                .unwrap_or_else(|error| panic!("encode missing-content event: {error}")),
        )
        .bind(valid.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("remove target content: {error}"));
    assert_rejection_code(
        store
            .set_local_lobby_message_pin(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &valid.id,
                false,
            )
            .await,
        "invalid_state",
    );
    assert_eq!(pin_count(&store).await, 1);
    assert_rejection_code(
        store
            .local_lobby_message_pins(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await,
        "invalid_state",
    );
    sqlx::query("UPDATE room_events SET event_json = '{}' WHERE room_id = 'general' AND seq = ?")
        .bind(valid.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("corrupt target event: {error}"));
    assert!(matches!(
        store
            .local_lobby_message_pins(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
            )
            .await,
        Err(PersistenceError::Json(_))
    ));
}

#[tokio::test]
async fn pin_limit_bounds_complete_list_without_blocking_repin_or_unpin() {
    let (store, principal) = fixture().await;
    let pin_limit = usize::try_from(MAX_LOBBY_MESSAGE_PINS)
        .unwrap_or_else(|error| panic!("convert pin limit: {error}"));
    let mut messages = Vec::new();
    for index in 0..=MAX_LOBBY_MESSAGE_PINS {
        messages.push(
            send(
                &store,
                &principal,
                &format!("message-limit-{index}"),
                &format!("bounded pin {index}"),
            )
            .await,
        );
    }
    for message in messages.iter().take(pin_limit) {
        store
            .set_local_lobby_message_pin(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &message.id,
                true,
            )
            .await
            .unwrap_or_else(|error| panic!("fill pin capacity: {error}"));
    }
    assert_eq!(pin_count(&store).await, MAX_LOBBY_MESSAGE_PINS);
    assert_rejection_code(
        store
            .set_local_lobby_message_pin(
                "general",
                LOCAL_OPERATOR_USER_ID,
                LOCAL_OPERATOR_PARTICIPANT_ID,
                &messages
                    .last()
                    .unwrap_or_else(|| panic!("extra message missing"))
                    .id,
                true,
            )
            .await,
        "pin_limit_reached",
    );
    let repinned = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &messages[0].id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("re-pin at capacity: {error}"));
    assert_eq!(repinned.len(), pin_limit);
    store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &messages[0].id,
            false,
        )
        .await
        .unwrap_or_else(|error| panic!("unpin at capacity: {error}"));
    let refilled = store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &messages
                .last()
                .unwrap_or_else(|| panic!("extra message missing"))
                .id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("refill pin capacity: {error}"));
    assert_eq!(refilled.len(), pin_limit);
}

#[tokio::test]
async fn human_session_permissions_and_revocation_are_rechecked_with_the_mutation() {
    let (read_only_store, local) = admitted_fixture(InviteScope::ReadOnly).await;
    let message = send(&read_only_store, &local, "read-only-target", "target").await;
    read_only_store
        .set_local_lobby_message_pin(
            "general",
            LOCAL_OPERATOR_USER_ID,
            LOCAL_OPERATOR_PARTICIPANT_ID,
            &message.id,
            true,
        )
        .await
        .unwrap_or_else(|error| panic!("seed readable pin: {error}"));
    let read_only = human_authorization(&read_only_store).await;
    assert_eq!(
        read_only_store
            .human_session_lobby_message_pins(&read_only)
            .await
            .unwrap_or_else(|error| panic!("read pins through read-only session: {error}"))
            .len(),
        1
    );
    assert_rejection_code(
        read_only_store
            .set_human_session_lobby_message_pin(&read_only, &message.id, false)
            .await,
        "permission_denied",
    );
    assert_eq!(pin_count(&read_only_store).await, 1);

    let (writable_store, local) = admitted_fixture(InviteScope::ReadWrite).await;
    let message = send(&writable_store, &local, "writable-target", "target").await;
    let writable = human_authorization(&writable_store).await;
    writable_store
        .set_human_session_lobby_message_pin(&writable, &message.id, true)
        .await
        .unwrap_or_else(|error| panic!("pin through writable session: {error}"));
    sqlx::query("UPDATE human_room_sessions SET state = 'ended'")
        .execute(&writable_store.pool)
        .await
        .unwrap_or_else(|error| panic!("end human session: {error}"));
    assert_rejection_code(
        writable_store
            .set_human_session_lobby_message_pin(&writable, &message.id, false)
            .await,
        "session_revoked",
    );
    assert_eq!(pin_count(&writable_store).await, 1);
}

async fn fixture() -> (SqliteStore, AuthenticatedPrincipal) {
    let store = SqliteStore::open(&format!(
        "sqlite:file:{}?mode=memory&cache=shared",
        uuid::Uuid::new_v4()
    ))
    .await
    .unwrap_or_else(|error| panic!("open store: {error}"));
    store
        .bootstrap_local_authority("113d2748-13cb-4310-ac4c-3bed54d19e6b", "Host")
        .await
        .unwrap_or_else(|error| panic!("bootstrap: {error}"));
    store
        .create_room_for_local_operator(
            "5568b5c4-b2e0-4217-a62a-30b2f07fbc70",
            "general",
            "General",
        )
        .await
        .unwrap_or_else(|error| panic!("create room: {error}"));
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

async fn admitted_fixture(invite_scope: InviteScope) -> (SqliteStore, AuthenticatedPrincipal) {
    let (store, principal) = fixture().await;
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'pin-guest', 'Pin Guest', ?, 1, 0, ?, 0, ?, ?)",
    )
    .bind(hex::encode(&SIGNED[..8]))
    .bind(SIGNED.as_slice())
    .bind(JOIN.as_slice())
    .bind(match invite_scope {
        InviteScope::ReadWrite => "read_write",
        InviteScope::ReadOnly => "read_only",
    })
    .bind((now + Duration::hours(2)).timestamp_micros())
    .bind(LOCAL_OPERATOR_USER_ID)
    .bind((now - Duration::minutes(1)).timestamp_micros())
    .execute(&store.pool)
    .await
    .unwrap_or_else(|error| panic!("insert invite: {error}"));
    let request = PreparedHumanAdmission::prepare(
        HumanInviteCredentialEvidence::JoinCode { fingerprint: JOIN },
        BROWSER,
        &HumanAdmissionInput {
            request_id: "d4250ad7-1ccc-4a04-bb5e-94260961459c".to_owned(),
            meeting_id_assertion: "general".to_owned(),
            display_name: "Pin Guest".to_owned(),
            participant_type: "human".to_owned(),
            owner_display_name: "Host".to_owned(),
            client_id: "pin-test-browser".to_owned(),
            avatar_image_url: String::new(),
        },
    )
    .unwrap_or_else(|error| panic!("prepare admission: {error}"));
    assert!(matches!(
        store
            .admit_human(&request, now)
            .await
            .unwrap_or_else(|error| panic!("admit human: {error}")),
        HumanAdmissionDecision::Admitted(_)
    ));
    (store, principal)
}

async fn human_authorization(store: &SqliteStore) -> crate::HumanSessionAuthorization {
    let fingerprint =
        sqlx::query_scalar::<_, Vec<u8>>("SELECT session_fingerprint FROM human_room_sessions")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read session fingerprint: {error}"))
            .try_into()
            .unwrap_or_else(|value: Vec<u8>| panic!("invalid fingerprint length: {}", value.len()));
    store
        .authorize_human_session(&fingerprint)
        .await
        .unwrap_or_else(|error| panic!("authorize human session: {error}"))
}

async fn pin_count(store: &SqliteStore) -> i64 {
    sqlx::query_scalar("SELECT COUNT(*) FROM room_message_pins")
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("count pins: {error}"))
}

fn assert_rejection_code<T>(result: Result<T, PersistenceError>, expected: &str) {
    match result {
        Err(PersistenceError::CommandRejected { code, .. }) => assert_eq!(code, expected),
        Err(error) => panic!("expected {expected}, got {error}"),
        Ok(_) => panic!("expected {expected} rejection"),
    }
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
        .unwrap_or_else(|error| panic!("send message: {error}"))
        .event
}
