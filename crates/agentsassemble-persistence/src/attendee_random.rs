use agentsassemble_domain::{RoomRandomRequest, RoomRandomResult, canonical_payload_hash};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{
    AttendeeConnectionAuthorization, AttendeeSessionAuthorization, CommandOutcome,
    PersistenceError, ProviderRoomRandomCommit, SqliteStore,
    attendee_invites::rejected,
    command_admission::{inspect_non_lifecycle_command, store_command_result},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttendeeRandomRequest {
    pub request_id: Uuid,
    pub turn_generation: u64,
    pub execution_id: String,
    pub action: String,
    pub payload: Value,
}

impl AttendeeRandomRequest {
    /// # Errors
    /// Rejects malformed or unsupported randomness requests using the canonical parser.
    pub fn parse(&self) -> Result<RoomRandomRequest, PersistenceError> {
        RoomRandomRequest::parse(&self.action, &self.payload)
            .map_err(|error| rejected("invalid_room_random_request", &error.message))
    }
}

pub struct AttendeeRandomMutation {
    pub outcome: CommandOutcome,
    pub result: RoomRandomResult,
}

impl SqliteStore {
    /// Commits one server-generated random result and its exact external retry receipt.
    ///
    /// # Errors
    /// Rejects stale custody/turns, changed retries, invalid results and exhausted tool budgets.
    pub async fn commit_attendee_random(
        &self,
        session: &AttendeeSessionAuthorization,
        connection_id: Uuid,
        request: &AttendeeRandomRequest,
        result: &RoomRandomResult,
        now: DateTime<Utc>,
    ) -> Result<AttendeeRandomMutation, PersistenceError> {
        if request.request_id.is_nil() {
            return Err(rejected("bad_request", "A tool request UUID is required."));
        }
        let connection = AttendeeConnectionAuthorization {
            session: session.clone(),
            connection_id,
        };
        let payload = serde_json::to_value(request)?;
        let hash = canonical_payload_hash(&payload);
        let request_id = request.request_id.to_string();
        let action = "bridge.tool.random";
        let principal = session.principal();
        let mut tx = self.pool.begin_with("BEGIN IMMEDIATE").await?;
        crate::attendee_connection::revalidate_in(&mut tx, &connection, now).await?;
        if let Some(outcome) = inspect_non_lifecycle_command(
            &mut tx,
            &principal.room_id,
            &principal.principal_id,
            &request_id,
            action,
            &hash,
        )
        .await?
        {
            let result = serde_json::from_value(outcome.result["tool_result"].clone())?;
            tx.commit().await?;
            return Ok(AttendeeRandomMutation { outcome, result });
        }
        let session = crate::attendee_tool_authority::load_turn_in(
            &mut tx,
            &connection,
            request.turn_generation,
            &request.execution_id,
        )
        .await?;
        let parsed = request.parse()?;
        let result_id = format!("result-{}", Uuid::new_v4().simple());
        let committed = crate::room_random::commit_provider_random_in(
            &mut tx,
            ProviderRoomRandomCommit {
                room_id: &principal.room_id,
                session_id: &session.public.session_id,
                turn_id: &session.public.active_turn_id,
                input_up_to_seq: session.input_up_to_seq,
                turn_generation: request.turn_generation,
                execution_id: &request.execution_id,
                result_id: &result_id,
                request: &parsed,
                result,
            },
            &session,
        )
        .await?;
        let event = committed.event;
        let result = json!({"event":event, "tool_result":committed.result});
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
        Ok(AttendeeRandomMutation {
            outcome: CommandOutcome {
                result,
                events: vec![event.clone()],
                event,
                deduplicated: false,
            },
            result: committed.result,
        })
    }
}
