use agentsassemble_persistence::{
    AttendeeConnectionAuthorization, AttendeeRuntimeReady, AttendeeTurnReport, PersistenceError,
    ProviderTurnStartAuthority,
};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{AppState, AttendeeOperation, AttendeeOperationResult};

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Request {
    Ready {
        request_id: Uuid,
        report: Box<AttendeeRuntimeReady>,
    },
    Started {
        request_id: Uuid,
        authority: Box<ProviderTurnStartAuthority>,
        provider_turn_id: String,
    },
    Report {
        report: Box<AttendeeTurnReport>,
    },
}

impl Request {
    pub(super) fn request_id(&self) -> Uuid {
        match self {
            Self::Ready { request_id, .. } | Self::Started { request_id, .. } => *request_id,
            Self::Report { report } => report.request_id,
        }
    }

    pub(super) async fn apply(
        self,
        state: &AppState,
        connection: &AttendeeConnectionAuthorization,
    ) -> Result<(Value, bool), PersistenceError> {
        let request_id = self.request_id();
        if request_id.is_nil() {
            return Err(PersistenceError::CommandRejected {
                code: "bad_request",
                message: "A request UUID is required.".to_owned(),
            });
        }
        let mut ready = false;
        let result = match self {
            Self::Ready { report, .. } => {
                let result = state
                    .rooms
                    .execute_attendee(AttendeeOperation::Ready {
                        connection: connection.clone(),
                        report,
                    })
                    .await?;
                ready = true;
                result
            }
            Self::Started {
                authority,
                provider_turn_id,
                ..
            } => {
                state
                    .store
                    .record_attendee_turn_started(
                        connection,
                        &authority,
                        &provider_turn_id,
                        chrono::Utc::now(),
                    )
                    .await?;
                AttendeeOperationResult::Applied
            }
            Self::Report { report } => {
                state
                    .rooms
                    .execute_attendee(AttendeeOperation::Report {
                        connection: connection.clone(),
                        report,
                    })
                    .await?
            }
        };
        let mut ack = json!({"type":"ack", "request_id":request_id, "resolution":"committed"});
        if let AttendeeOperationResult::Reported {
            event_id,
            sequence,
            deduplicated,
        } = result
        {
            ack["event_id"] = json!(event_id);
            ack["sequence"] = json!(sequence);
            ack["deduplicated"] = json!(deduplicated);
        }
        Ok((ack, ready))
    }
}

pub(super) fn failure(request_id: Uuid, error: PersistenceError) -> Value {
    let failure = crate::room_command_result::CommandFailure::transactional(error);
    let code = match failure.error {
        PersistenceError::CommandRejected { code, .. }
        | PersistenceError::CommandUnresolved { code, .. } => code,
        PersistenceError::CommandConflict => "command_conflict",
        _ => "attendee_operation_failed",
    };
    // No provider payload, runtime token or raw storage diagnostic crosses this boundary.
    json!({"type":"nack", "request_id":request_id, "resolution":failure.resolution, "error":{"code":code}})
}
