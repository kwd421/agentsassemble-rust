use agentsassemble_persistence::{
    AgentTurnCommit, AttendeeCleanupAuthorization, AttendeeCleanupReport,
    AttendeeConnectionAuthorization, AttendeeRuntimeReady, AttendeeSessionAuthorization,
    AttendeeTurnReport, PersistenceError,
};
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::event_publication::PublicationAttempt;

pub enum AttendeeOperation {
    Leave {
        authority: AttendeeCleanupAuthorization,
        request_id: Uuid,
    },
    Interrupt {
        authority: AttendeeCleanupAuthorization,
        connection_id: Uuid,
        report: Box<agentsassemble_persistence::AttendeeInterruptReport>,
    },
    Connect {
        session: AttendeeSessionAuthorization,
        connection_id: Uuid,
    },
    Ready {
        connection: AttendeeConnectionAuthorization,
        report: Box<AttendeeRuntimeReady>,
    },
    Report {
        connection: AttendeeConnectionAuthorization,
        report: Box<AttendeeTurnReport>,
    },
    Cleanup {
        authority: AttendeeCleanupAuthorization,
        report: Box<AttendeeCleanupReport>,
    },
    Disconnect {
        connection: AttendeeConnectionAuthorization,
    },
}

impl AttendeeOperation {
    fn room_id(&self) -> &str {
        match self {
            Self::Cleanup { authority, .. }
            | Self::Interrupt { authority, .. }
            | Self::Leave { authority, .. } => authority.room_id(),
            Self::Connect { session, .. } => &session.principal().room_id,
            Self::Ready { connection, .. }
            | Self::Report { connection, .. }
            | Self::Disconnect { connection } => &connection.session().principal().room_id,
        }
    }
}

pub enum AttendeeOperationResult {
    Connected(AttendeeConnectionAuthorization),
    Applied,
    Reported {
        event_id: String,
        sequence: i64,
        deduplicated: bool,
    },
}

pub(super) struct AttendeeCommand {
    operation: AttendeeOperation,
    reply: oneshot::Sender<Result<AttendeeOperationResult, PersistenceError>>,
}

impl RoomRuntime {
    /// Serializes attendee connection, readiness and results through canonical room publication.
    ///
    /// # Errors
    /// Returns provenance, queue, transaction and unresolved response failures.
    pub async fn execute_attendee(
        &self,
        operation: AttendeeOperation,
    ) -> Result<AttendeeOperationResult, PersistenceError> {
        let handle = self.handle(operation.room_id()).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Attendee(AttendeeCommand { operation, reply }))
            .map_err(|error| {
                let (code, message) = match error {
                    mpsc::error::TrySendError::Full(_) => {
                        ("room_busy", "Room mutation queue is full.")
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        ("room_unavailable", "Room mutation task stopped.")
                    }
                };
                PersistenceError::CommandRejected {
                    code,
                    message: message.to_owned(),
                }
            })?;
        response.await.map_err(|_| PersistenceError::CommandUnresolved {
            code: "attendee_response_lost", message: "The attendee operation response was lost; recover its exact connection or report identity.".to_owned(),
        })?
    }
}

pub(super) async fn execute(
    owners: RoomCommandOwners<'_>,
    command: AttendeeCommand,
) -> Option<PublicationAttempt> {
    let result = apply(owners.store, command.operation).await;
    let (reply, publication) = match result {
        Ok((reply, commit)) => {
            let publication = crate::provider_turn::publish_turn_commit(
                owners.store,
                owners.event_tx,
                owners.turn_tasks,
                owners.provider_adapter.clone(),
                owners.room_tool_ingress.clone(),
                owners.attachment_ingress.clone(),
                commit,
            )
            .await;
            (Ok(reply), publication)
        }
        Err(error) => (Err(error), None),
    };
    let _ = command.reply.send(reply);
    publication
}

async fn apply(
    store: &agentsassemble_persistence::SqliteStore,
    operation: AttendeeOperation,
) -> Result<(AttendeeOperationResult, AgentTurnCommit), PersistenceError> {
    let now = chrono::Utc::now();
    let mut empty = AgentTurnCommit {
        events: Vec::new(),
        next_assignments: Vec::new(),
    };
    match operation {
        AttendeeOperation::Leave {
            authority,
            request_id,
        } => Ok(reported(
            store.leave_attendee(&authority, request_id).await?,
        )),
        AttendeeOperation::Interrupt {
            authority,
            connection_id,
            report,
        } => {
            let mutation = store
                .record_attendee_interrupt(&authority, connection_id, &report, now)
                .await?;
            Ok(reported(mutation))
        }
        AttendeeOperation::Connect {
            session,
            connection_id,
        } => {
            let claim = store
                .claim_attendee_connection(&session, connection_id, now)
                .await?;
            empty.events = claim.events;
            Ok((
                AttendeeOperationResult::Connected(claim.authorization),
                empty,
            ))
        }
        AttendeeOperation::Ready { connection, report } => {
            let commit = store
                .record_attendee_ready(&connection, &report, now)
                .await?;
            Ok((AttendeeOperationResult::Applied, commit))
        }
        AttendeeOperation::Report { connection, report } => {
            let mutation = store
                .record_attendee_turn_report(&connection, &report, now)
                .await?;
            Ok(reported(mutation))
        }
        AttendeeOperation::Cleanup { authority, report } => {
            let mutation = store.record_attendee_cleanup(&authority, &report).await?;
            Ok(reported(mutation))
        }
        AttendeeOperation::Disconnect { connection } => {
            empty.events = store
                .disconnect_attendee_connection(&connection, now)
                .await?
                .into_iter()
                .collect();
            Ok((AttendeeOperationResult::Applied, empty))
        }
    }
}

fn reported(
    mutation: agentsassemble_persistence::RoomCommandMutation,
) -> (AttendeeOperationResult, AgentTurnCommit) {
    (
        AttendeeOperationResult::Reported {
            event_id: mutation.outcome.event.id,
            sequence: mutation.outcome.event.seq,
            deduplicated: mutation.outcome.deduplicated,
        },
        AgentTurnCommit {
            events: mutation.outcome.events,
            next_assignments: mutation.assignments,
        },
    )
}
