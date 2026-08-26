use std::collections::BTreeMap;

use agentsassemble_domain::{
    Actor, AuthenticatedPrincipal, Participant, ParticipantStatus, RoomEvent,
    canonical_payload_hash,
};
use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlx::{Row, Sqlite, Transaction};
use uuid::Uuid;

use crate::{
    CommandOutcome, HumanSessionAuthorization, PersistenceError, SqliteStore,
    agent_lifecycle_events::store_result,
    authority::active_room_for_principal,
    command_admission::inspect_non_lifecycle_command,
    human_session_authority::{fixed_session_fingerprint, revalidate_human_session},
    room_turns::support::{insert_event, load_participant, next_sequence},
};

pub(crate) const PARTICIPANT_LEAVE_ACTION: &str = "participant.leave";

#[derive(Debug, Clone)]
pub struct ParticipantLeaveMutation {
    pub outcome: CommandOutcome,
    pub revoked_session_fingerprints: Vec<[u8; 32]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParticipantLeave {}

impl SqliteStore {
    /// Rejects the local room owner while preserving the same command identity rules.
    ///
    /// # Errors
    ///
    /// Returns authorization, payload, replay, or owner-transfer failures.
    pub async fn execute_participant_leave(
        &self,
        principal: &AuthenticatedPrincipal,
        request_id: &str,
        payload: &Value,
    ) -> Result<ParticipantLeaveMutation, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        active_room_for_principal(&mut transaction, principal).await?;
        let mutation =
            execute_leave_in(&mut transaction, principal, request_id, payload, None).await?;
        transaction.commit().await?;
        Ok(mutation)
    }

    /// Atomically leaves one admitted human and ends its exact live room session.
    ///
    /// # Errors
    ///
    /// Returns authorization, payload, replay, membership, or stored-state failures.
    pub async fn execute_human_session_participant_leave(
        &self,
        authorization: &HumanSessionAuthorization,
        request_id: &str,
        payload: &Value,
    ) -> Result<ParticipantLeaveMutation, PersistenceError> {
        let mut transaction = self.pool.begin().await?;
        let (current, _) =
            revalidate_human_session(&mut transaction, authorization, Utc::now()).await?;
        let mutation = execute_leave_in(
            &mut transaction,
            current.principal(),
            request_id,
            payload,
            Some(current.session_fingerprint()),
        )
        .await?;
        transaction.commit().await?;
        Ok(mutation)
    }
}

async fn execute_leave_in(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
    session_fingerprint: Option<&[u8; 32]>,
) -> Result<ParticipantLeaveMutation, PersistenceError> {
    if !principal.capabilities.participant_leave {
        return Err(rejected(
            "permission_denied",
            "This room session cannot leave the room.",
        ));
    }
    if principal.is_operator {
        return Err(rejected(
            "owner_must_transfer_or_delete",
            "The room owner must transfer ownership or delete the server.",
        ));
    }
    let payload_hash = canonical_payload_hash(payload);
    if inspect_non_lifecycle_command(
        transaction,
        &principal.room_id,
        &principal.principal_id,
        request_id,
        PARTICIPANT_LEAVE_ACTION,
        &payload_hash,
    )
    .await?
    .is_some()
    {
        // A committed leave makes its own session unusable. Reaching the same
        // request from a joined session means that identifier belongs to an
        // earlier membership lifetime and must not revoke the new session.
        return Err(PersistenceError::CommandConflict);
    }
    parse_payload(payload)?;
    let mut participant =
        load_participant(transaction, &principal.room_id, &principal.participant_id).await?;
    require_exact_joined_human(&participant, principal)?;
    participant.status = ParticipantStatus::Left;
    participant.updated_at = Utc::now();
    let revoked_session_fingerprints = match session_fingerprint {
        Some(fingerprint) => end_exact_session(transaction, principal, fingerprint).await?,
        None => Vec::new(),
    };
    sqlx::query(
        "UPDATE participants SET participant_json = ? WHERE room_id = ? AND participant_id = ?",
    )
    .bind(serde_json::to_string(&participant)?)
    .bind(&principal.room_id)
    .bind(&principal.participant_id)
    .execute(&mut **transaction)
    .await?;
    let event = participant_left_event(transaction, principal, &participant).await?;
    insert_event(transaction, &event).await?;
    let result = json!({
        "participant": participant,
        "event": event,
        "event_seq": event.seq,
    });
    let outcome = store_result(
        transaction,
        principal,
        request_id,
        PARTICIPANT_LEAVE_ACTION,
        payload_hash,
        result,
        vec![event],
    )
    .await?;
    Ok(ParticipantLeaveMutation {
        outcome,
        revoked_session_fingerprints,
    })
}

fn parse_payload(payload: &Value) -> Result<ParticipantLeave, PersistenceError> {
    serde_json::from_value(payload.clone()).map_err(|_| {
        rejected(
            "invalid_participant_leave",
            "participant.leave requires an empty object.",
        )
    })
}

fn require_exact_joined_human(
    participant: &Participant,
    principal: &AuthenticatedPrincipal,
) -> Result<(), PersistenceError> {
    if participant.room_id != principal.room_id
        || participant.participant_id != principal.participant_id
        || participant.participant_type != "human"
    {
        return Err(rejected(
            "invalid_state",
            "Stored leave participant authority is invalid.",
        ));
    }
    if participant.status != ParticipantStatus::Joined {
        return Err(rejected("session_revoked", "This room session has ended."));
    }
    Ok(())
}

async fn end_exact_session(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    fingerprint: &[u8; 32],
) -> Result<Vec<[u8; 32]>, PersistenceError> {
    let rows = sqlx::query(
        "UPDATE human_room_sessions SET state = 'ended' WHERE session_fingerprint = ? AND room_id = ? AND user_id = ? AND participant_id = ? AND state = 'active' RETURNING session_fingerprint",
    )
    .bind(fingerprint.as_slice())
    .bind(&principal.room_id)
    .bind(&principal.principal_id)
    .bind(&principal.participant_id)
    .fetch_all(&mut **transaction)
    .await?;
    if rows.len() != 1 {
        return Err(rejected(
            "invalid_state",
            "The exact human room session could not be ended.",
        ));
    }
    rows.into_iter()
        .map(|row| fixed_session_fingerprint(row.get("session_fingerprint")))
        .collect()
}

async fn participant_left_event(
    transaction: &mut Transaction<'_, Sqlite>,
    principal: &AuthenticatedPrincipal,
    participant: &Participant,
) -> Result<RoomEvent, PersistenceError> {
    Ok(RoomEvent {
        v: 1,
        id: Uuid::new_v4().to_string(),
        seq: next_sequence(transaction, &principal.room_id).await?,
        created_at: Utc::now(),
        room_id: principal.room_id.clone(),
        event_type: "participant_left".to_owned(),
        actor: Actor {
            participant_id: principal.participant_id.clone(),
            participant_type: "human".to_owned(),
        },
        participant_id: Some(participant.participant_id.clone()),
        participant_type: Some("human".to_owned()),
        actor_id: Some(principal.participant_id.clone()),
        actor_type: Some("human".to_owned()),
        display_name: Some(participant.display_name.clone()),
        content: None,
        message_kind: None,
        extra: BTreeMap::new(),
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
        LOCAL_OPERATOR_PARTICIPANT_ID, LOCAL_OPERATOR_USER_ID, Participant, ParticipantStatus,
    };
    use chrono::{Duration, Utc};
    use serde_json::json;

    use crate::{
        HumanAdmissionDecision, HumanAdmissionInput, HumanInviteCredentialEvidence,
        PersistenceError, PreparedHumanAdmission, SqliteStore,
    };

    use super::PARTICIPANT_LEAVE_ACTION;

    const JOIN: [u8; 32] = [0x61; 32];
    const SIGNED: [u8; 32] = [0x62; 32];
    const BROWSER: [u8; 32] = [0x63; 32];
    const REJOIN_JOIN: [u8; 32] = [0x64; 32];
    const REJOIN_SIGNED: [u8; 32] = [0x65; 32];

    #[tokio::test]
    async fn read_only_leave_is_one_unbudgeted_membership_and_session_transaction() {
        let (store, authorization, fingerprint) = admitted_fixture(InviteScope::ReadOnly).await;
        let payload = json!({});
        let participant_id = authorization.principal().participant_id.clone();
        assert!(
            !store
                .command_requires_principal_budget(
                    authorization.principal(),
                    "leave-request",
                    PARTICIPANT_LEAVE_ACTION,
                    &payload,
                )
                .await
                .unwrap_or_else(|error| panic!("inspect leave budget: {error}"))
        );

        let mutation = store
            .execute_human_session_participant_leave(&authorization, "leave-request", &payload)
            .await
            .unwrap_or_else(|error| panic!("leave room: {error}"));
        assert_eq!(mutation.revoked_session_fingerprints, vec![fingerprint]);
        assert_eq!(mutation.outcome.event.event_type, "participant_left");
        assert_eq!(mutation.outcome.result["participant"]["status"], "left");

        let participant: Participant = serde_json::from_str(
            &sqlx::query_scalar::<_, String>(
                "SELECT participant_json FROM participants WHERE room_id = 'general' AND participant_id = ?",
            )
            .bind(&participant_id)
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read left participant: {error}")),
        )
        .unwrap_or_else(|error| panic!("decode left participant: {error}"));
        assert_eq!(participant.status, ParticipantStatus::Left);
        assert_eq!(
            sqlx::query_scalar::<_, String>(
                "SELECT state FROM human_room_sessions WHERE session_fingerprint = ?",
            )
            .bind(fingerprint.as_slice())
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("read ended session: {error}")),
            "ended"
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM command_results WHERE action = 'participant.leave'",
            )
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count leave result: {error}")),
            1
        );
        assert!(matches!(
            store.authorize_human_session(&fingerprint).await,
            Err(PersistenceError::CommandRejected {
                code: "session_revoked",
                ..
            })
        ));
        let rejoined = rejoin(&store).await;
        assert!(
            store
                .command_requires_principal_budget(
                    rejoined.principal(),
                    "leave-request",
                    PARTICIPANT_LEAVE_ACTION,
                    &payload,
                )
                .await
                .unwrap_or_else(|error| panic!("inspect old leave identity budget: {error}"))
        );
        assert!(matches!(
            store
                .execute_human_session_participant_leave(&rejoined, "leave-request", &payload,)
                .await,
            Err(PersistenceError::CommandConflict)
        ));
        store
            .authorize_human_session(rejoined.session_fingerprint())
            .await
            .unwrap_or_else(|error| panic!("old leave identity revoked rejoined session: {error}"));
    }

    #[tokio::test]
    async fn owner_and_nonempty_payload_fail_without_partial_leave() {
        let (store, authorization, _) = admitted_fixture(InviteScope::ReadWrite).await;
        assert!(
            store
                .command_requires_principal_budget(
                    authorization.principal(),
                    "bad-leave",
                    PARTICIPANT_LEAVE_ACTION,
                    &json!({"participant_id": "someone-else"}),
                )
                .await
                .unwrap_or_else(|error| panic!("inspect invalid leave budget: {error}"))
        );
        assert!(matches!(
            store
                .execute_human_session_participant_leave(
                    &authorization,
                    "bad-leave",
                    &json!({"participant_id": "someone-else"}),
                )
                .await,
            Err(PersistenceError::CommandRejected {
                code: "invalid_participant_leave",
                ..
            })
        ));
        store
            .authorize_human_session(authorization.session_fingerprint())
            .await
            .unwrap_or_else(|error| panic!("invalid payload revoked session: {error}"));

        let owner = AuthenticatedPrincipal {
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
        assert!(
            store
                .command_requires_principal_budget(
                    &owner,
                    "owner-leave",
                    PARTICIPANT_LEAVE_ACTION,
                    &json!({}),
                )
                .await
                .unwrap_or_else(|error| panic!("inspect owner leave budget: {error}"))
        );
        assert!(matches!(
            store
                .execute_participant_leave(&owner, "owner-leave", &json!({}))
                .await,
            Err(PersistenceError::CommandRejected {
                code: "owner_must_transfer_or_delete",
                ..
            })
        ));
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM room_events WHERE event_json LIKE '%participant_left%'",
            )
            .fetch_one(&store.pool)
            .await
            .unwrap_or_else(|error| panic!("count partial leave events: {error}")),
            0
        );
    }

    async fn admitted_fixture(
        scope: InviteScope,
    ) -> (SqliteStore, crate::HumanSessionAuthorization, [u8; 32]) {
        let store = SqliteStore::open("sqlite::memory:")
            .await
            .unwrap_or_else(|error| panic!("open leave fixture: {error}"));
        store
            .bootstrap_local_authority("a88a5e82-2566-4e2f-967f-c06ea12443d9", "Host")
            .await
            .unwrap_or_else(|error| panic!("bootstrap leave fixture: {error}"));
        store
            .create_room_for_local_operator(
                "2e625e28-67c0-4864-9905-e5f513608c5a",
                "general",
                "General",
            )
            .await
            .unwrap_or_else(|error| panic!("create leave room: {error}"));
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'leave-guest', 'Leave Guest', ?, 0, 0, ?, 0, ?, ?)",
        )
        .bind(hex::encode(&SIGNED[..8]))
        .bind(SIGNED.as_slice())
        .bind(JOIN.as_slice())
        .bind(match scope {
            InviteScope::ReadWrite => "read_write",
            InviteScope::ReadOnly => "read_only",
        })
        .bind((now + Duration::hours(1)).timestamp_micros())
        .bind(LOCAL_OPERATOR_USER_ID)
        .bind((now - Duration::minutes(1)).timestamp_micros())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert leave invite: {error}"));
        let request = PreparedHumanAdmission::prepare(
            HumanInviteCredentialEvidence::JoinCode { fingerprint: JOIN },
            BROWSER,
            &HumanAdmissionInput {
                request_id: "f07ac460-3f61-4b93-86f8-2925c792fe6d".to_owned(),
                meeting_id_assertion: "general".to_owned(),
                display_name: "Leave Guest".to_owned(),
                participant_type: "human".to_owned(),
                owner_display_name: "Host".to_owned(),
                client_id: "leave-browser".to_owned(),
                avatar_image_url: String::new(),
            },
        )
        .unwrap_or_else(|error| panic!("prepare leave admission: {error}"));
        assert!(matches!(
            store
                .admit_human(&request, now)
                .await
                .unwrap_or_else(|error| panic!("admit leave guest: {error}")),
            HumanAdmissionDecision::Admitted(_)
        ));
        let fingerprint: [u8; 32] = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT session_fingerprint FROM human_room_sessions WHERE state = 'active'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read leave fingerprint: {error}"))
        .try_into()
        .unwrap_or_else(|value: Vec<u8>| panic!("invalid fingerprint length: {}", value.len()));
        let authorization = store
            .authorize_human_session(&fingerprint)
            .await
            .unwrap_or_else(|error| panic!("authorize leave guest: {error}"));
        (store, authorization, fingerprint)
    }

    async fn rejoin(store: &SqliteStore) -> crate::HumanSessionAuthorization {
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO room_invites(invite_id, signed_token_fingerprint, join_code_fingerprint, room_id, base_participant_id, display_name, invite_scope, max_uses, use_count, expires_at, revoked, created_by_user_id, created_at) VALUES (?, ?, ?, 'general', 'unused', 'Leave Guest', 'read_only', 0, 0, ?, 0, ?, ?)",
        )
        .bind(hex::encode(&REJOIN_SIGNED[..8]))
        .bind(REJOIN_SIGNED.as_slice())
        .bind(REJOIN_JOIN.as_slice())
        .bind((now + Duration::hours(1)).timestamp_micros())
        .bind(LOCAL_OPERATOR_USER_ID)
        .bind(now.timestamp_micros())
        .execute(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("insert rejoin invite: {error}"));
        let request = PreparedHumanAdmission::prepare(
            HumanInviteCredentialEvidence::JoinCode {
                fingerprint: REJOIN_JOIN,
            },
            BROWSER,
            &HumanAdmissionInput {
                request_id: "1d87cc04-6cbb-4409-8b10-f54057c47cbd".to_owned(),
                meeting_id_assertion: "general".to_owned(),
                display_name: "Leave Guest".to_owned(),
                participant_type: "human".to_owned(),
                owner_display_name: "Host".to_owned(),
                client_id: "leave-browser".to_owned(),
                avatar_image_url: String::new(),
            },
        )
        .unwrap_or_else(|error| panic!("prepare rejoin: {error}"));
        assert!(matches!(
            store
                .admit_human(&request, now)
                .await
                .unwrap_or_else(|error| panic!("rejoin leave guest: {error}")),
            HumanAdmissionDecision::Admitted(_)
        ));
        let fingerprint: [u8; 32] = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT session_fingerprint FROM human_room_sessions WHERE state = 'active'",
        )
        .fetch_one(&store.pool)
        .await
        .unwrap_or_else(|error| panic!("read rejoined fingerprint: {error}"))
        .try_into()
        .unwrap_or_else(|value: Vec<u8>| panic!("invalid fingerprint length: {}", value.len()));
        store
            .authorize_human_session(&fingerprint)
            .await
            .unwrap_or_else(|error| panic!("authorize rejoined guest: {error}"))
    }
}
