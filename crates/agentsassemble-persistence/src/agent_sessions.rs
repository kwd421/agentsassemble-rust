use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AgentSession, AgentSessionDraft, AuthenticatedPrincipal, ClientKind, Participant,
    ParticipantStatus, RoomEvent, canonical_payload_hash,
};
use chrono::Utc;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore, authority::active_room_for_principal,
    sqlite::existing_command,
};

impl SqliteStore {
    /// Returns a committed result before a caller consults mutable external selection state.
    ///
    /// # Errors
    ///
    /// Returns a conflict, inactive-session rejection, or persistence failure.
    pub async fn replay_command(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        action: &str,
        payload: &Value,
    ) -> Result<Option<CommandOutcome>, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let outcome = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            action,
            &payload_hash,
        )
        .await?;
        transaction.commit().await?;
        Ok(outcome)
    }

    /// Atomically creates a stopped, server-owned Agent Session and its room event.
    ///
    /// # Errors
    ///
    /// Returns authorization, identity, idempotency, or persistence failures.
    #[allow(clippy::too_many_lines)] // One transaction must keep records, event, and ACK cohesive.
    pub async fn execute_agent_create(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
        draft: &AgentSessionDraft,
    ) -> Result<CommandOutcome, PersistenceError> {
        const ACTION: &str = "agent.create";
        if principal.client_kind == ClientKind::AgentBridge || !principal.capabilities.agent_control
        {
            return Err(PersistenceError::CommandRejected {
                code: "permission_denied",
                message: "agent.control permission is required.".to_owned(),
            });
        }
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if let Some(outcome) = existing_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &payload_hash,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let collision = sqlx::query_scalar::<_, i64>(
            "SELECT (SELECT COUNT(*) FROM participants WHERE room_id = ? AND participant_id = ?) + (SELECT COUNT(*) FROM agent_sessions WHERE room_id = ? AND session_id = ?)",
        )
        .bind(&principal.room_id)
        .bind(&draft.agent_id)
        .bind(&principal.room_id)
        .bind(&draft.agent_id)
        .fetch_one(&mut *transaction)
        .await?;
        if collision != 0 {
            return Err(PersistenceError::CommandRejected {
                code: "session_exists",
                message: "An Agent Session with this identity already exists.".to_owned(),
            });
        }
        let now = Utc::now();
        let participant = Participant {
            room_id: principal.room_id.clone(),
            participant_id: draft.agent_id.clone(),
            display_name: draft.display_name.clone(),
            participant_type: "agent".to_owned(),
            status: ParticipantStatus::Detached,
            role: "agent".to_owned(),
            owner_id: principal.principal_id.clone(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        let (last_message_id, last_message_seq) =
            latest_message_cursor(&mut transaction, &principal.room_id).await?;
        let session = AgentSession {
            room_id: principal.room_id.clone(),
            session_id: draft.agent_id.clone(),
            participant_id: draft.agent_id.clone(),
            display_name: draft.display_name.clone(),
            status: "available".to_owned(),
            runtime_status: "stopped".to_owned(),
            enabled: false,
            provider_kind: draft.provider_kind.clone(),
            runtime_kind: draft.runtime_kind.clone(),
            connection_kind: "native_cli_bridge".to_owned(),
            external_owned: false,
            process_ownership: "server".to_owned(),
            workspace: draft.workspace.clone(),
            model: draft.model.clone(),
            reasoning_effort: draft.reasoning_effort.clone(),
            service_tier: draft.service_tier.clone(),
            variant: draft.variant.clone(),
            execution_harness: draft.execution_harness.clone(),
            permission_mode: draft.permission_mode.clone(),
            max_output_tokens: draft.max_output_tokens,
            catalog_revision: draft.catalog_revision.clone(),
            runtime_profile_key: draft.runtime_profile_key.clone(),
            transport: draft.transport.clone(),
            last_seen_event_id: last_message_id.clone(),
            last_seen_seq: last_message_seq,
            last_provider_sync_event_id: last_message_id,
            last_provider_sync_seq: last_message_seq,
            bootstrap_cutoff_seq: last_message_seq,
            turn_count: 0,
            created_at: now,
            updated_at: now,
        };
        sqlx::query(
            "INSERT INTO participants(room_id, participant_id, participant_json) VALUES (?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&participant.participant_id)
        .bind(serde_json::to_string(&participant)?)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT INTO agent_sessions(room_id, session_id, session_json) VALUES (?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&session.session_id)
        .bind(serde_json::to_string(&session)?)
        .execute(&mut *transaction)
        .await?;
        let sequence = next_sequence(&mut transaction, &principal.room_id).await?;
        let mut extra = BTreeMap::new();
        extra.insert("session_id".to_owned(), json!(session.session_id));
        extra.insert("provider_kind".to_owned(), json!(session.provider_kind));
        let event = RoomEvent {
            v: 1,
            id: Uuid::new_v4().to_string(),
            seq: sequence,
            created_at: now,
            room_id: principal.room_id.clone(),
            event_type: "agent_session_created".to_owned(),
            actor: Actor {
                participant_id: principal.participant_id.clone(),
                participant_type: "human".to_owned(),
            },
            participant_id: Some(session.participant_id.clone()),
            participant_type: Some("agent".to_owned()),
            actor_id: Some(principal.participant_id.clone()),
            actor_type: Some("human".to_owned()),
            display_name: Some(session.display_name.clone()),
            content: None,
            message_kind: None,
            relay_depth: None,
            extra,
        };
        sqlx::query("INSERT INTO room_events(room_id, seq, event_json) VALUES (?, ?, ?)")
            .bind(&principal.room_id)
            .bind(sequence)
            .bind(serde_json::to_string(&event)?)
            .execute(&mut *transaction)
            .await?;
        let result = json!({
            "status": "created",
            "agent_session": session,
            "participant": participant,
            "event_seq": sequence,
            "event": event,
        });
        sqlx::query(
            "INSERT INTO command_results(room_id, principal_id, request_id, action, payload_hash, result_json) VALUES (?, ?, ?, ?, ?, ?)",
        )
        .bind(&principal.room_id)
        .bind(&principal.principal_id)
        .bind(request_id)
        .bind(ACTION)
        .bind(payload_hash)
        .bind(serde_json::to_string(&result)?)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(CommandOutcome {
            result,
            event,
            deduplicated: false,
        })
    }
}

async fn latest_message_cursor(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
) -> Result<(String, i64), PersistenceError> {
    let event_json = sqlx::query_scalar::<_, String>(
        "SELECT event_json FROM room_events WHERE room_id = ? AND json_extract(event_json, '$.type') = 'message_final' ORDER BY seq DESC LIMIT 1",
    )
    .bind(room_id)
    .fetch_optional(&mut **transaction)
    .await?;
    event_json.map_or_else(
        || Ok((String::new(), 0)),
        |event_json| {
            let event: RoomEvent = serde_json::from_str(&event_json)?;
            Ok((event.id, event.seq))
        },
    )
}

async fn next_sequence(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    room_id: &str,
) -> Result<i64, PersistenceError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM room_events WHERE room_id = ?",
    )
    .bind(room_id)
    .fetch_one(&mut **transaction)
    .await?)
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AgentSessionDraft, AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, Participant, ParticipantStatus, Room, RoomSettings,
    };
    use chrono::Utc;
    use serde_json::json;

    use crate::{PersistenceError, SqliteStore};

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        let now = Utc::now();
        let room = Room::new("general".to_owned(), "General".to_owned(), now);
        let participant = Participant {
            room_id: "general".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Host".to_owned(),
            participant_type: "human".to_owned(),
            status: ParticipantStatus::Joined,
            role: "host".to_owned(),
            owner_id: String::new(),
            muted: false,
            created_at: now,
            updated_at: now,
        };
        store
            .initialize_room(
                &room,
                &RoomSettings::defaults("General".to_owned()),
                &participant,
            )
            .await
            .unwrap_or_else(|error| panic!("initialize room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
            participant_id: LOCAL_OPERATOR_PARTICIPANT_ID.to_owned(),
            display_name: "Host".to_owned(),
            room_id: "general".to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            capabilities: CapabilitySet::local_operator(
                ClientKind::Browser,
                InviteScope::ReadWrite,
            ),
        };
        (store, principal, directory)
    }

    fn draft(workspace: &str) -> AgentSessionDraft {
        AgentSessionDraft {
            agent_id: "codex-00000000-0000-5000-8000-000000000001".to_owned(),
            display_name: "Terra".to_owned(),
            provider_kind: "codex_live_session".to_owned(),
            runtime_kind: "live_cli".to_owned(),
            executable: "codex".to_owned(),
            workspace: workspace.to_owned(),
            model: "gpt-5.6-terra".to_owned(),
            reasoning_effort: "medium".to_owned(),
            service_tier: "default".to_owned(),
            variant: String::new(),
            execution_harness: "builtin".to_owned(),
            permission_mode: "meeting_read_only".to_owned(),
            max_output_tokens: 0,
            catalog_revision: "catalog-1".to_owned(),
            runtime_profile_key: "profile-1".to_owned(),
            transport: "stdio_jsonl".to_owned(),
        }
    }

    #[tokio::test]
    async fn create_replay_and_snapshot_are_one_durable_identity() {
        let (store, principal, directory) = fixture().await;
        let payload = json!({"provider_id": "codex", "catalog_revision": "catalog-1"});
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let session = draft(workspace);
        let first = store
            .execute_agent_create(&principal, "create-1", &payload, &session)
            .await
            .unwrap_or_else(|error| panic!("create session: {error}"));
        let retry = store
            .execute_agent_create(&principal, "create-1", &payload, &session)
            .await
            .unwrap_or_else(|error| panic!("retry session: {error}"));
        assert!(!first.deduplicated);
        assert!(retry.deduplicated);
        assert_eq!(first.event.id, retry.event.id);
        assert!(matches!(
            store
                .replay_command(
                    &principal,
                    "create-1",
                    "agent.create",
                    &json!({"provider_id": "changed"})
                )
                .await,
            Err(PersistenceError::CommandConflict)
        ));
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert_eq!(snapshot.agent_sessions[0].model, "gpt-5.6-terra");
        assert_eq!(snapshot.events[0].event_type, "agent_session_created");
    }

    #[tokio::test]
    async fn command_result_failure_rolls_back_session_participant_and_event() {
        let (store, principal, directory) = fixture().await;
        sqlx::query(
            "CREATE TRIGGER reject_agent_result BEFORE INSERT ON command_results BEGIN SELECT RAISE(ABORT, 'injected failure'); END",
        )
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("install trigger: {error}"));
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let result = store
            .execute_agent_create(
                &principal,
                "create-fails",
                &json!({"provider_id": "codex"}),
                &draft(workspace),
            )
            .await;
        assert!(result.is_err());
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert!(snapshot.agent_sessions.is_empty());
        assert_eq!(snapshot.participants.len(), 1);
        assert!(snapshot.events.is_empty());
    }

    #[tokio::test]
    async fn missing_agent_control_rejects_without_partial_authority() {
        let (store, mut principal, directory) = fixture().await;
        principal.capabilities.agent_control = false;
        let workspace = directory
            .path()
            .to_str()
            .unwrap_or_else(|| panic!("test workspace path must be UTF-8"));
        let result = store
            .execute_agent_create(
                &principal,
                "create-denied",
                &json!({"provider_id": "codex"}),
                &draft(workspace),
            )
            .await;
        assert!(matches!(
            result,
            Err(PersistenceError::CommandRejected {
                code: "permission_denied",
                ..
            })
        ));
        let snapshot = store
            .snapshot("general", 0, 200)
            .await
            .unwrap_or_else(|error| panic!("snapshot: {error}"));
        assert!(snapshot.agent_sessions.is_empty());
        assert_eq!(snapshot.participants.len(), 1);
        assert!(snapshot.events.is_empty());
    }
}
