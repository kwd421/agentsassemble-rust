use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, ParticipantRole, RoomEvent, canonical_payload_hash,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    CommandOutcome, PersistenceError, SqliteStore,
    authority::active_room_for_principal,
    command_admission::{admit_non_lifecycle_command, store_command_result},
    room_event_sequence::next_sequence,
    room_turns::support::{insert_event, load_participant},
    room_write_budget::command_size,
};

const ACTION: &str = "participant.role.update";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleUpdate {
    participant_id: String,
    role: ParticipantRole,
}

impl SqliteStore {
    /// Atomically updates one room-owned participant role and its public result.
    ///
    /// # Errors
    ///
    /// Returns authorization, payload, target, replay, or storage failures.
    pub async fn execute_participant_role_update(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let payload_hash = canonical_payload_hash(payload);
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        if !principal.capabilities.room_manage {
            return Err(rejected(
                "permission_denied",
                "This room session cannot manage participant roles.",
            ));
        }
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut transaction,
            &principal.room_id,
            &principal.principal_id,
            request_id,
            ACTION,
            &payload_hash,
            command_size(request_id, ACTION, payload)?,
        )
        .await?
        {
            transaction.commit().await?;
            return Ok(outcome);
        }
        let update = parse_update(payload)?;
        let mut participant =
            load_participant(&mut transaction, &principal.room_id, &update.participant_id)
                .await
                .map_err(|error| match error {
                    PersistenceError::ParticipantMissing => rejected(
                        "invalid_participant_role",
                        "The participant does not exist in this room.",
                    ),
                    other => other,
                })?;
        if participant.room_id != principal.room_id
            || participant.participant_id != update.participant_id
        {
            return Err(rejected(
                "invalid_state",
                "Stored participant identity does not match its room key.",
            ));
        }
        participant.role = update.role;
        participant.updated_at = Utc::now();
        let event = role_updated_event(&mut transaction, principal, &participant).await?;
        let result = json!({
            "participant": participant,
            "event": event,
            "event_seq": event.seq,
        });
        sqlx::query(
            "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
        )
        .bind(serde_json::to_string(&participant)?)
        .bind(&principal.room_id)
        .bind(&participant.participant_id)
        .execute(&mut *transaction)
        .await?;
        insert_event(&mut transaction, &event).await?;
        store_command_result(
            &mut transaction,
            principal,
            request_id,
            ACTION,
            &payload_hash,
            &result,
        )
        .await?;
        transaction.commit().await?;
        Ok(CommandOutcome {
            result,
            event: event.clone(),
            events: vec![event],
            deduplicated: false,
        })
    }
}

fn parse_update(payload: &Value) -> Result<RoleUpdate, PersistenceError> {
    let update = serde_json::from_value::<RoleUpdate>(payload.clone()).map_err(|_| {
        rejected(
            "invalid_participant_role",
            "participant.role.update requires one participant_id and one supported role.",
        )
    })?;
    if update.participant_id.is_empty()
        || update.participant_id.len() > 128
        || update.participant_id.trim() != update.participant_id
        || update.participant_id.chars().any(char::is_control)
    {
        return Err(rejected(
            "invalid_participant_role",
            "participant_id is invalid.",
        ));
    }
    Ok(update)
}

async fn role_updated_event(
    transaction: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &agentsassemble_domain::Participant,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: Utc::now(),
        room_id: principal.room_id.clone(),
        event_type: "participant_updated".to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some(participant.participant_type.clone()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::from([("role".to_owned(), json!(participant.role))]),
    })
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{
        AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope,
        LOCAL_OPERATOR_PARTICIPANT_ID, ParticipantRole,
    };
    use serde_json::json;

    use crate::{PersistenceError, SqliteStore};

    #[tokio::test]
    async fn role_update_is_one_room_owned_replayable_transition() {
        let (store, principal, _directory) = fixture().await;
        let payload = json!({
            "participant_id": LOCAL_OPERATOR_PARTICIPANT_ID,
            "role": "director",
        });
        let first = store
            .execute_participant_role_update(&principal, "role-1", &payload)
            .await
            .unwrap_or_else(|error| panic!("update participant role: {error}"));
        assert!(!first.deduplicated);
        assert_eq!(first.event.event_type, "participant_updated");
        assert_eq!(
            first.event.extra.get("role"),
            Some(&json!(ParticipantRole::Director))
        );
        assert_eq!(
            first.result["participant"]["role"],
            json!(ParticipantRole::Director)
        );

        let stored = store
            .participant("general", LOCAL_OPERATOR_PARTICIPANT_ID)
            .await
            .unwrap_or_else(|error| panic!("read updated participant: {error}"));
        assert_eq!(stored.role, ParticipantRole::Director);
        let snapshot = store
            .snapshot("general", 0, 20)
            .await
            .unwrap_or_else(|error| panic!("read role snapshot: {error}"));
        assert_eq!(snapshot.participants[0].role, ParticipantRole::Director);

        let replay = store
            .execute_participant_role_update(&principal, "role-1", &payload)
            .await
            .unwrap_or_else(|error| panic!("replay participant role: {error}"));
        assert!(replay.deduplicated);
        assert_eq!(replay.event.id, first.event.id);
        assert!(matches!(
            store
                .execute_participant_role_update(
                    &principal,
                    "role-1",
                    &json!({
                        "participant_id": LOCAL_OPERATOR_PARTICIPANT_ID,
                        "role": "reviewer",
                    }),
                )
                .await,
            Err(PersistenceError::CommandConflict)
        ));
    }

    #[tokio::test]
    async fn role_update_rejects_aliases_unknown_targets_and_missing_room_authority() {
        let (store, principal, _directory) = fixture().await;
        assert!(matches!(
            store
                .execute_participant_role_update(
                    &principal,
                    "role-alias",
                    &json!({
                        "participant_id": LOCAL_OPERATOR_PARTICIPANT_ID,
                        "role": "host",
                    }),
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "invalid_participant_role",
                ..
            })
        ));
        assert!(matches!(
            store
                .execute_participant_role_update(
                    &principal,
                    "role-missing",
                    &json!({"participant_id": "missing", "role": "agent"}),
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "invalid_participant_role",
                ..
            })
        ));

        let mut guest = principal;
        guest.is_operator = false;
        guest.capabilities =
            CapabilitySet::for_principal(ClientKind::Browser, InviteScope::ReadWrite, false);
        assert!(matches!(
            store
                .execute_participant_role_update(
                    &guest,
                    "role-denied",
                    &json!({
                        "participant_id": LOCAL_OPERATOR_PARTICIPANT_ID,
                        "role": "reviewer",
                    }),
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "permission_denied",
                ..
            })
        ));
    }

    async fn fixture() -> (SqliteStore, AuthenticatedPrincipal, tempfile::TempDir) {
        let directory =
            tempfile::tempdir().unwrap_or_else(|error| panic!("create test directory: {error}"));
        let store = SqliteStore::open_path(&directory.path().join("runtime.sqlite3"))
            .await
            .unwrap_or_else(|error| panic!("open store: {error}"));
        store
            .bootstrap_local_authority("42aebf93-31ce-46fd-b792-0a791b644668", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap identity: {error}"));
        store
            .create_room_for_local_operator(
                "20000000-0000-4000-8000-000000000010",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create room: {error}"));
        let principal = AuthenticatedPrincipal {
            principal_id: "operator-local-user".to_owned(),
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
        (store, principal, directory)
    }
}
