use super::{AgentTurnCommit, ProviderTurnAuthority, RoomCommandMutation, completion};
use crate::{
    AttendeeConnectionAuthorization, CommandOutcome, PersistenceError, SqliteStore,
    command_admission::{admit_non_lifecycle_command, store_command_result},
};
use agentsassemble_domain::{VoteCommand, canonical_payload_hash};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

/// Exact client-owned execution identity; room and participant come only from admission.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeTurnReport {
    pub request_id: Uuid,
    pub turn_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub start_dispatch_nonce: String,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
    pub provider_turn_id: String,
    pub provider_session_id: Option<String>,
    pub outcome: AttendeeTurnOutcome,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AttendeeTurnOutcome {
    Message {
        content: String,
        target_agent_id: String,
    },
    Vote {
        payload: Value,
    },
    Declined {
        reason_code: String,
    },
    // Raw provider diagnostics belong to the external client, never public room history.
    Failed {
        error_code: String,
    },
}

impl SqliteStore {
    /// Commits an authenticated external result and its retry receipt with canonical turn effects.
    ///
    /// # Errors
    /// Rejects stale connections/executions, changed retries, revoked admission and invalid outcomes.
    pub async fn record_attendee_turn_report(
        &self,
        connection: &AttendeeConnectionAuthorization,
        report: &AttendeeTurnReport,
        now: DateTime<Utc>,
    ) -> Result<RoomCommandMutation, PersistenceError> {
        if report.request_id.is_nil() {
            return Err(super::support::rejected(
                "bad_request",
                "A report UUID is required.",
            ));
        }
        let payload = serde_json::to_value(report)?;
        let hash = canonical_payload_hash(&payload);
        let request_id = report.request_id.to_string();
        let action = "bridge.turn.report";
        let bytes = crate::room_write_budget::command_size(&request_id, action, &payload)?;
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, connection, now).await?;
        let principal = connection.session.principal();
        if let Some(outcome) = admit_non_lifecycle_command(
            &mut tx,
            &principal.room_id,
            &principal.principal_id,
            &request_id,
            action,
            &hash,
            bytes,
        )
        .await?
        {
            tx.commit().await?;
            return Ok(RoomCommandMutation {
                outcome,
                assignments: Vec::new(),
            });
        }
        let commit = apply_report(&mut tx, connection, report).await?;
        let event = commit.events.first().cloned().ok_or_else(|| {
            super::support::rejected(
                "invalid_state",
                "A terminal report produced no canonical event.",
            )
        })?;
        let result = json!({"event": event, "events": commit.events});
        store_command_result(
            &mut tx,
            (&principal.room_id, &principal.principal_id),
            &request_id,
            action,
            &hash,
            &result,
        )
        .await?;
        tx.commit().await?;
        Ok(RoomCommandMutation {
            outcome: CommandOutcome {
                result,
                event,
                events: commit.events,
                deduplicated: false,
            },
            assignments: commit.next_assignments,
        })
    }
}

async fn apply_report(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    connection: &AttendeeConnectionAuthorization,
    report: &AttendeeTurnReport,
) -> Result<AgentTurnCommit, PersistenceError> {
    let principal = connection.session.principal();
    let session =
        crate::agent_lifecycle::load_session(tx, &principal.room_id, &principal.participant_id)
            .await?;
    let execution = crate::provider_turn_execution::load_execution_in(
        tx,
        &principal.room_id,
        &session.public.session_id,
        report.turn_generation,
    )
    .await?;
    if !execution.provider_turn_id.is_empty()
        && execution.provider_turn_id != report.provider_turn_id
    {
        return Err(super::support::rejected(
            "stale_provider_turn",
            "The reported provider turn does not match its execution.",
        ));
    }
    let authority = ProviderTurnAuthority {
        room_id: &principal.room_id,
        session_id: &session.public.session_id,
        turn_id: &report.turn_id,
        turn_generation: report.turn_generation,
        execution_id: &report.execution_id,
        start_dispatch_nonce: &report.start_dispatch_nonce,
        runtime_handle_id: &report.runtime_handle_id,
        runtime_owner_id: &report.runtime_owner_id,
        runtime_lease_token: &report.runtime_lease_token,
        provider_turn_id: &report.provider_turn_id,
        provider_session_id: report.provider_session_id.as_deref(),
    };
    let room = &principal.room_id;
    let session_id = &session.public.session_id;
    match &report.outcome {
        AttendeeTurnOutcome::Message {
            content,
            target_agent_id,
        } => {
            completion::complete_message(tx, room, session_id, authority, content, target_agent_id)
                .await
        }
        AttendeeTurnOutcome::Vote { payload } => {
            let command = VoteCommand::from_payload(payload).map_err(super::support::rejection)?;
            completion::complete_vote(tx, room, session_id, authority, command).await
        }
        AttendeeTurnOutcome::Declined { reason_code } => {
            completion::decline(tx, room, session_id, authority, reason_code).await
        }
        AttendeeTurnOutcome::Failed { error_code } => {
            completion::fail(
                tx,
                room,
                session_id,
                authority,
                error_code,
                "External provider turn failed.",
                None,
            )
            .await
        }
    }
}
