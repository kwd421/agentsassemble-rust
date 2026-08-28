use agentsassemble_domain::{LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, RoomEvent};
use chrono::{DateTime, SecondsFormat, Utc};
use serde_json::Value;
use sqlx::{Row, Sqlite, Transaction};

use crate::{
    HumanSessionAuthorization, PersistenceError, SqliteStore,
    human_session_authority::revalidate_human_session,
    room_user_identity::{require_current_local_room_manager, resolve_room_user_identity},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinnedLobbyMessage {
    pub event_id: String,
    pub pinned_at: String,
    pub seq: i64,
    pub author: String,
    pub content: String,
    pub created_at: String,
    pub attachment_filenames: Vec<String>,
}

impl SqliteStore {
    /// Lists lobby pins while the canonical local operator remains this room's manager.
    ///
    /// # Errors
    ///
    /// Fails when local authority, a stored pointer, its event, or persistence is invalid.
    pub async fn local_lobby_message_pins(
        &self,
        room_id: &str,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_local_operator(&mut transaction, room_id).await?;
        let pins = load_pins(&mut transaction, room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Lists lobby pins while an exact durable human session retains room-history permission.
    ///
    /// # Errors
    ///
    /// Fails when session authority, permission, a stored pointer, its event, or persistence is
    /// invalid.
    pub async fn human_session_lobby_message_pins(
        &self,
        expected: &HumanSessionAuthorization,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_permission(
            principal.capabilities.room_history,
            "This room session cannot read message history.",
        )?;
        let pins = load_pins(&mut transaction, &principal.room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Pins or unpins one lobby message as the canonical local operator.
    ///
    /// # Errors
    ///
    /// Fails without writing when local authority or the target message is invalid.
    pub async fn set_local_lobby_message_pin(
        &self,
        room_id: &str,
        event_id: &str,
        pinned: bool,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        authorize_local_operator(&mut transaction, room_id).await?;
        set_pin(&mut transaction, room_id, event_id, pinned, Utc::now()).await?;
        let pins = load_pins(&mut transaction, room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }

    /// Pins or unpins one lobby message while an exact durable human session remains writable.
    ///
    /// # Errors
    ///
    /// Fails without writing when session authority, permission, or the target message is invalid.
    pub async fn set_human_session_lobby_message_pin(
        &self,
        expected: &HumanSessionAuthorization,
        event_id: &str,
        pinned: bool,
    ) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) = revalidate_human_session(&mut transaction, expected, Utc::now()).await?;
        let principal = current.principal();
        require_permission(
            principal.capabilities.message_modify,
            "This room session cannot modify messages.",
        )?;
        set_pin(
            &mut transaction,
            &principal.room_id,
            event_id,
            pinned,
            Utc::now(),
        )
        .await?;
        let pins = load_pins(&mut transaction, &principal.room_id).await?;
        transaction.commit().await?;
        Ok(pins)
    }
}

async fn authorize_local_operator(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<(), PersistenceError> {
    let identity = resolve_room_user_identity(
        transaction,
        room_id,
        LOCAL_OPERATOR_USER_ID,
        LOCAL_OPERATOR_PARTICIPANT_ID,
    )
    .await?;
    require_current_local_room_manager(transaction, &identity).await?;
    Ok(())
}

fn require_permission(allowed: bool, message: &'static str) -> Result<(), PersistenceError> {
    if allowed {
        Ok(())
    } else {
        Err(rejected("permission_denied", message))
    }
}

async fn set_pin(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
    pinned: bool,
    now: DateTime<Utc>,
) -> Result<(), PersistenceError> {
    validate_event_id(event_id)?;
    if !pinned {
        sqlx::query("DELETE FROM room_message_pins WHERE room_id = ? AND event_id = ?")
            .bind(room_id)
            .bind(event_id)
            .execute(&mut **transaction)
            .await?;
        return Ok(());
    }
    let event = load_target_message(transaction, room_id, event_id).await?;
    sqlx::query(
        "INSERT INTO room_message_pins(room_id, event_id, event_seq, pinned_at) VALUES (?, ?, ?, ?) ON CONFLICT(room_id, event_id) DO UPDATE SET event_seq = excluded.event_seq, pinned_at = excluded.pinned_at",
    )
    .bind(room_id)
    .bind(event_id)
    .bind(event.seq)
    .bind(now.timestamp_micros())
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn load_target_message(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
    event_id: &str,
) -> Result<RoomEvent, PersistenceError> {
    let rows = sqlx::query(
        "SELECT seq, event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.id') = ? LIMIT 2",
    )
    .bind(room_id)
    .bind(event_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() != 1 {
        return Err(if rows.is_empty() {
            rejected("message_missing", "The message was not found.")
        } else {
            invalid_state("Stored room event identity is not unique.")
        });
    }
    let row = &rows[0];
    let seq = row.get::<i64, _>("seq");
    let event: RoomEvent = serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
    require_message_event(&event, room_id, event_id, seq)?;
    Ok(event)
}

async fn load_pins(
    transaction: &mut Transaction<'_, Sqlite>,
    room_id: &str,
) -> Result<Vec<PinnedLobbyMessage>, PersistenceError> {
    let rows = sqlx::query(
        "SELECT pins.event_id, pins.event_seq, pins.pinned_at, events.event_json FROM room_message_pins AS pins JOIN room_events AS events ON events.room_id = pins.room_id AND events.seq = pins.event_seq WHERE pins.room_id = ? ORDER BY pins.pinned_at DESC, pins.event_id ASC",
    )
    .bind(room_id)
    .fetch_all(&mut **transaction)
    .await?;
    rows.into_iter()
        .map(|row| {
            let event_id = row.get::<String, _>("event_id");
            let event_seq = row.get::<i64, _>("event_seq");
            let pinned_at = row.get::<i64, _>("pinned_at");
            let event: RoomEvent =
                serde_json::from_str(row.get::<String, _>("event_json").as_str())?;
            require_message_event(&event, room_id, &event_id, event_seq)?;
            project_pin(event, pinned_at)
        })
        .collect()
}

fn project_pin(
    event: RoomEvent,
    pinned_at_micros: i64,
) -> Result<PinnedLobbyMessage, PersistenceError> {
    let pinned_at = DateTime::from_timestamp_micros(pinned_at_micros)
        .ok_or_else(|| invalid_state("Stored message pin timestamp is invalid."))?;
    let author = event
        .display_name
        .filter(|name| !name.is_empty())
        .or_else(|| (!event.actor.participant_id.is_empty()).then_some(event.actor.participant_id))
        .unwrap_or_else(|| "Room".to_owned());
    Ok(PinnedLobbyMessage {
        event_id: event.id,
        pinned_at: pinned_at.to_rfc3339_opts(SecondsFormat::AutoSi, true),
        seq: event.seq,
        author,
        content: event.content.unwrap_or_default(),
        created_at: event
            .created_at
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
        attachment_filenames: Vec::new(),
    })
}

fn require_message_event(
    event: &RoomEvent,
    room_id: &str,
    event_id: &str,
    seq: i64,
) -> Result<(), PersistenceError> {
    if event.room_id != room_id || event.id != event_id || event.seq != seq {
        return Err(invalid_state("Stored message pin target is inconsistent."));
    }
    if event.event_type != "message_final"
        || event.extra.get("message_deleted") == Some(&Value::Bool(true))
    {
        return Err(rejected("message_missing", "The message was not found."));
    }
    Ok(())
}

fn validate_event_id(event_id: &str) -> Result<(), PersistenceError> {
    if event_id.is_empty() || event_id.len() > 128 || event_id.contains('\0') {
        return Err(rejected("bad_request", "event_id is invalid."));
    }
    Ok(())
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

fn invalid_state(message: impl Into<String>) -> PersistenceError {
    rejected("invalid_state", message)
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID,
    };
    use chrono::{Duration, Utc};
    use serde_json::json;

    use crate::{
        HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
        PersistenceError, PreparedHumanAdmission, SqliteStore,
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
            .set_local_lobby_message_pin("general", &first.id, true)
            .await
            .unwrap_or_else(|error| panic!("pin first: {error}"));
        sqlx::query("UPDATE room_message_pins SET pinned_at = 1 WHERE event_id = ?")
            .bind(&first.id)
            .execute(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("age first pin: {error}"));
        let pins = store
            .set_local_lobby_message_pin("general", &second.id, true)
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
            .set_local_lobby_message_pin("general", &first.id, true)
            .await
            .unwrap_or_else(|error| panic!("re-pin first: {error}"));
        assert_eq!(repinned.len(), 2);
        assert_eq!(repinned[0].event_id, first.id);
        let remaining = store
            .set_local_lobby_message_pin("general", &second.id, false)
            .await
            .unwrap_or_else(|error| panic!("unpin second: {error}"));
        assert_eq!(remaining.len(), 1);
        let unchanged = store
            .set_local_lobby_message_pin("general", &second.id, false)
            .await
            .unwrap_or_else(|error| panic!("repeat unpin: {error}"));
        assert_eq!(unchanged, remaining);
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

        for event_id in ["missing", room_created.id.as_str(), "bad\0id"] {
            assert!(
                store
                    .set_local_lobby_message_pin("general", event_id, true)
                    .await
                    .is_err()
            );
        }
        let valid = send(&store, &principal, "message-valid", "valid").await;
        let before = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM room_message_pins")
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count pins: {error}"));
        assert_eq!(before, 0);

        store
            .set_local_lobby_message_pin("general", &valid.id, true)
            .await
            .unwrap_or_else(|error| panic!("pin valid message: {error}"));
        sqlx::query(
            "UPDATE room_events SET event_json = '{}' WHERE room_id = 'general' AND seq = ?",
        )
        .bind(valid.seq)
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("corrupt target event: {error}"));
        assert!(matches!(
            store.local_lobby_message_pins("general").await,
            Err(PersistenceError::Json(_))
        ));
    }

    #[tokio::test]
    async fn human_session_permissions_and_revocation_are_rechecked_with_the_mutation() {
        let (read_only_store, local) = admitted_fixture(InviteScope::ReadOnly).await;
        let message = send(&read_only_store, &local, "read-only-target", "target").await;
        read_only_store
            .set_local_lobby_message_pin("general", &message.id, true)
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
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
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
                .unwrap_or_else(|value: Vec<u8>| {
                    panic!("invalid fingerprint length: {}", value.len())
                });
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
}
