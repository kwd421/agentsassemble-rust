pub(super) use crate::attendee_wire::AttendeeSocketRequest as Request;
use crate::attendee_wire::{AttendeeSocketFailure, AttendeeSocketFrame as Frame};
use agentsassemble_persistence::{AttendeeConnectionAuthorization, PersistenceError};
use uuid::Uuid;

use crate::{AppState, AttendeeOperation, AttendeeOperationResult};

impl Request {
    pub(super) async fn apply(
        self,
        state: &AppState,
        connection: &AttendeeConnectionAuthorization,
    ) -> Result<(Frame, bool), PersistenceError> {
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
        let (event_id, sequence, deduplicated) = match result {
            AttendeeOperationResult::Reported {
                event_id,
                sequence,
                deduplicated,
            } => (Some(event_id), Some(sequence), Some(deduplicated)),
            _ => (None, None, None),
        };
        let ack = Frame::Ack {
            request_id,
            resolution: agentsassemble_protocol::CommandResolution::Committed,
            event_id,
            sequence,
            deduplicated,
        };
        Ok((ack, ready))
    }
}

pub(super) fn failure(request_id: Uuid, error: PersistenceError) -> Frame {
    let failure = crate::room_command_result::CommandFailure::transactional(error);
    let code = match failure.error {
        PersistenceError::CommandRejected { code, .. }
        | PersistenceError::CommandUnresolved { code, .. } => code,
        PersistenceError::CommandConflict => "command_conflict",
        _ => "attendee_operation_failed",
    };
    // No provider payload, runtime token or raw storage diagnostic crosses this boundary.
    Frame::Nack {
        request_id,
        resolution: failure.resolution,
        error: AttendeeSocketFailure {
            code: code.to_owned(),
        },
    }
}
