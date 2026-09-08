use agentsassemble_persistence::{
    ConnectorAdmission, ConnectorSessionAuthorization, PersistenceError,
};
use agentsassemble_protocol::RoomAction;
use serde_json::Value;
use tokio::sync::{mpsc, oneshot};
use uuid::Uuid;

use super::{RoomCommand, RoomCommandOwners, RoomCommandSession, RoomMutation, RoomRuntime};
use crate::{
    event_publication::{PublicationAttempt, publish_durable_room_events},
    room_command_admission::admit_current_command,
    room_command_execution::CommandExecution,
    room_command_result::{CommandFailure, validate_command_envelope},
};

pub(super) struct ConnectorAdmissionCommand {
    invite_fingerprint: [u8; 32],
    client_fingerprint: [u8; 32],
    request_id: Uuid,
    display_name: String,
    reply: oneshot::Sender<Result<ConnectorAdmission, PersistenceError>>,
}

impl RoomRuntime {
    /// Commits external AI membership through its room's bounded mutation/publication owner.
    ///
    /// # Errors
    /// Rejects unavailable invitations, lost queue custody or failed admission transactions.
    pub async fn admit_connector(
        &self,
        invite_fingerprint: [u8; 32],
        client_fingerprint: [u8; 32],
        request_id: Uuid,
        display_name: String,
    ) -> Result<ConnectorAdmission, PersistenceError> {
        let room_id = self
            .store
            .connector_admission_room_id(&invite_fingerprint)
            .await?
            .ok_or_else(|| {
                rejected(
                    "invite_unavailable",
                    "The connector invitation is unavailable.",
                )
            })?;
        let handle = self.handle(&room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::ConnectorAdmission(
                ConnectorAdmissionCommand {
                    invite_fingerprint,
                    client_fingerprint,
                    request_id,
                    display_name,
                    reply,
                },
            ))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => {
                    rejected("room_busy", "Room mutation queue is full.")
                }
                mpsc::error::TrySendError::Closed(_) => {
                    rejected("room_unavailable", "Room mutation task stopped.")
                }
            })?;
        response
            .await
            .map_err(|_| rejected("room_unavailable", "Room admission response was lost."))?
    }

    pub(crate) async fn execute_connector(
        &self,
        expected: &ConnectorSessionAuthorization,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<agentsassemble_persistence::CommandOutcome, CommandFailure> {
        validate_command_envelope(&request_id).map_err(CommandFailure::rejected)?;
        if !matches!(
            action,
            RoomAction::MessageSend
                | RoomAction::RoomRandomRoll
                | RoomAction::RoomRandomChoose
                | RoomAction::ParticipantLeave
        ) {
            return Err(CommandFailure::rejected(rejected(
                "permission_denied",
                "This action is not a Room Connector operation.",
            )));
        }
        let current = self
            .store
            .revalidate_connector_session(expected, chrono::Utc::now())
            .await
            .map_err(CommandFailure::unresolved)?;
        let admitted = admit_current_command(
            &self.store,
            &self.principal_mutations,
            current.principal().clone(),
            &request_id,
            action,
            &payload,
        )
        .await?;
        let room_uid = current.room_uid();
        self.enqueue_command(
            admitted,
            Some(RoomCommandSession::Connector(Box::new(current))),
            Some(room_uid),
            request_id,
            action,
            payload,
        )
        .await
    }
}

pub(super) async fn admit(
    owners: &RoomCommandOwners<'_>,
    room_id: &str,
    command: ConnectorAdmissionCommand,
) -> Option<PublicationAttempt> {
    let admission = owners
        .store
        .admit_connector(
            &command.invite_fingerprint,
            &command.client_fingerprint,
            command.request_id,
            &command.display_name,
            chrono::Utc::now(),
        )
        .await;
    let publication = if admission.is_ok() {
        Some(publish_durable_room_events(owners.store, owners.event_tx, room_id).await)
    } else {
        None
    };
    let _ = command.reply.send(admission);
    publication
}

pub(crate) async fn execute(
    store: &agentsassemble_persistence::SqliteStore,
    command: &RoomCommand,
    authorization: &ConnectorSessionAuthorization,
) -> CommandExecution {
    match command.action {
        RoomAction::MessageSend => match store
            .execute_authorized_message_with_turn(
                command.mutation_authority(),
                &command.request_id,
                command.action.as_str(),
                &command.payload,
            )
            .await
        {
            Ok(mutation) => CommandExecution::mutation(mutation),
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::RoomRandomRoll | RoomAction::RoomRandomChoose => {
            match crate::room_random_runtime::execute_authorized_room_random(
                store,
                command,
                command.mutation_authority(),
            )
            .await
            {
                Ok(outcome) => CommandExecution::success(outcome),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        RoomAction::ParticipantLeave
            if command
                .payload
                .as_object()
                .is_some_and(serde_json::Map::is_empty) =>
        {
            match store
                .leave_connector_session(authorization, &command.request_id)
                .await
            {
                Ok(mutation) => CommandExecution::participant_leave(mutation),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        _ => CommandExecution::transactional_failure(rejected(
            "permission_denied",
            "This action is not a Room Connector operation.",
        )),
    }
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}
