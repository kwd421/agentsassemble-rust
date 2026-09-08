use super::{RoomMutation, RoomRuntime};
use crate::{
    principal_mutation_admission::MutationDebit,
    room_command_admission::{
        AdmittedHumanCommand, admit_human_command, admit_room_session_command,
    },
    room_command_result::{CommandFailure, public_command_outcome},
};
use agentsassemble_domain::AuthenticatedPrincipal;
use agentsassemble_persistence::{
    CommandOutcome, ConnectorSessionAuthorization, PersistenceError, RoomSessionAuthorization,
};
use agentsassemble_protocol::RoomAction;
use serde_json::Value;
use tokio::sync::{OwnedSemaphorePermit, mpsc, oneshot};

pub(crate) enum RoomCommandSession {
    Browser(Box<RoomSessionAuthorization>),
    Connector(Box<ConnectorSessionAuthorization>),
}

pub(crate) struct RoomCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) room_uid: Option<uuid::Uuid>,
    pub(crate) session: Option<RoomCommandSession>,
    pub(crate) request_id: String,
    pub(crate) action: RoomAction,
    pub(crate) payload: Value,
    pub(super) mutation_debit: Option<MutationDebit>,
    _inflight_permit: OwnedSemaphorePermit,
    pub(super) reply: oneshot::Sender<Result<CommandOutcome, CommandFailure>>,
}

impl RoomCommand {
    pub(crate) fn mutation_authority(
        &self,
    ) -> agentsassemble_persistence::RoomMutationAuthority<'_> {
        match &self.session {
            Some(RoomCommandSession::Browser(session)) => session.mutation_authority(),
            Some(RoomCommandSession::Connector(session)) => {
                agentsassemble_persistence::RoomMutationAuthority::ConnectorSession(session)
            }
            None => {
                agentsassemble_persistence::RoomMutationAuthority::TrustedPrincipal(&self.principal)
            }
        }
    }
}

impl RoomRuntime {
    /// Enqueues one durable command on its room owner and classifies its outcome.
    pub(crate) async fn execute(
        &self,
        principal: AuthenticatedPrincipal,
        room_uid: Option<uuid::Uuid>,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let admitted = admit_human_command(
            &self.store,
            &self.principal_mutations,
            &principal,
            &request_id,
            action,
            &payload,
        )
        .await?;
        self.enqueue_command(admitted, None, room_uid, request_id, action, payload)
            .await
    }

    pub(crate) async fn execute_room_session(
        &self,
        authorization: &RoomSessionAuthorization,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let (admitted, current) = admit_room_session_command(
            &self.store,
            &self.principal_mutations,
            authorization,
            &request_id,
            action,
            &payload,
        )
        .await?;
        self.enqueue_command(
            admitted,
            Some(RoomCommandSession::Browser(Box::new(current))),
            None,
            request_id,
            action,
            payload,
        )
        .await
    }

    pub(super) async fn enqueue_command(
        &self,
        admitted: AdmittedHumanCommand,
        session: Option<RoomCommandSession>,
        room_uid: Option<uuid::Uuid>,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let AdmittedHumanCommand {
            principal,
            mutation_debit,
            inflight_permit,
        } = admitted;
        let handle = if action == RoomAction::RoomDelete {
            // Serialize replay routing with deletion retirement. An immutable
            // retry must not recreate an actor for a physically absent room.
            let mut rooms = self.rooms.lock().await;
            if session.is_none()
                && let Some(outcome) = self
                    .store
                    .completed_room_deletion(&principal, &request_id, &payload)
                    .await
                    .map_err(CommandFailure::unresolved)?
            {
                if let Some(debit) = &mutation_debit {
                    debit.resolve();
                }
                return public_command_outcome(&principal, outcome)
                    .map_err(CommandFailure::unresolved);
            }
            self.handle_locked(&principal.room_id, &mut rooms).await
        } else {
            self.handle(&principal.room_id).await
        };
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Command(RoomCommand {
                principal,
                room_uid,
                session,
                request_id,
                action,
                payload,
                mutation_debit,
                _inflight_permit: inflight_permit,
                reply,
            }))
            .map_err(|error| {
                CommandFailure::unresolved(match error {
                    mpsc::error::TrySendError::Full(_) => PersistenceError::CommandRejected {
                        code: "room_busy",
                        message: "Room command queue is full.".to_owned(),
                    },
                    mpsc::error::TrySendError::Closed(_) => PersistenceError::CommandRejected {
                        code: "room_unavailable",
                        message: "Room mutation task stopped.".to_owned(),
                    },
                })
            })?;
        response.await.map_err(|_| {
            CommandFailure::unresolved(PersistenceError::CommandRejected {
                code: "room_unavailable",
                message: "Room mutation response was lost.".to_owned(),
            })
        })?
    }
}
