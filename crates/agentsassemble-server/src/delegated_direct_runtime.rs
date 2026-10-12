use agentsassemble_domain::{DirectRoomAction, canonical_payload_hash};
use agentsassemble_persistence::{
    CommandOutcome, DelegateRoomAuthority, PersistenceError, SqliteStore,
};
use serde_json::json;
use tokio::sync::{OwnedSemaphorePermit, oneshot};

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};
use crate::room_command_execution::CommandExecution;

pub(super) struct DelegatedCommand {
    authority: DelegateRoomAuthority,
    request: DirectRoomAction,
    admission: PrincipalMutationAdmission,
    _inflight: OwnedSemaphorePermit,
    reply: oneshot::Sender<Result<CommandOutcome, PersistenceError>>,
}

impl RoomRuntime {
    /// Executes an action on the room actor, retaining moderation effects and floor progression.
    /// # Errors
    /// Rejects ended custody, budget exhaustion, conflicting retries and invalid targets.
    pub async fn execute_delegated_direct(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DirectRoomAction,
    ) -> Result<CommandOutcome, PersistenceError> {
        let inflight = self.principal_mutations.acquire_inflight()?;
        let handle = self.handle(authority.room_id()).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Delegated(DelegatedCommand {
                authority: authority.clone(),
                request: request.clone(),
                admission: self.principal_mutations.clone(),
                _inflight: inflight,
                reply,
            }))
            .map_err(|error| PersistenceError::CommandRejected {
                code: if matches!(error, tokio::sync::mpsc::error::TrySendError::Full(_)) {
                    "room_busy"
                } else {
                    "room_unavailable"
                }
                .into(),
                message: "The room mutation queue is unavailable.".into(),
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "delegated_response_lost".into(),
                message: "The action response was lost; retry the exact request.".into(),
            })?
    }
}

pub(super) async fn execute(
    owners: RoomCommandOwners<'_>,
    command: DelegatedCommand,
) -> Option<crate::event_publication::PublicationAttempt> {
    let execution = apply(
        owners.store,
        owners.provider_adapter,
        &command.admission,
        &command.authority,
        &command.request,
    )
    .await;
    let publication = publish(owners, &execution).await;
    let _ = command
        .reply
        .send(execution.reply.map_err(|failure| failure.error));
    publication
}

pub(crate) async fn publish(
    owners: RoomCommandOwners<'_>,
    execution: &CommandExecution,
) -> Option<crate::event_publication::PublicationAttempt> {
    crate::provider_turn::publish_turn_commit(
        owners.store,
        owners.event_tx,
        owners.turn_tasks,
        owners.provider_adapter.clone(),
        owners.ingress.clone(),
        agentsassemble_persistence::AgentTurnCommit {
            events: execution.committed_events.clone(),
            next_assignments: execution.assignments.clone(),
        },
    )
    .await
}

pub(crate) async fn apply(
    store: &SqliteStore,
    provider: &agentsassemble_provider::ProviderAdapter,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
) -> CommandExecution {
    let result = store
        .execute_delegated_atomic(authority, request, |owner, delegate, bytes| {
            let hash = canonical_payload_hash(&json!({"delegate":delegate,"request":request}));
            let identity = MutationIdentity::new(
                authority.room_id(),
                request.request_id(),
                request.name(),
                &hash,
            );
            Ok([
                admission.charge(owner, identity, bytes)?,
                admission.charge(delegate, identity, bytes)?,
            ])
        })
        .await;
    match result {
        Ok((mutation, debits)) => {
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            crate::room_command_dispatch::finish_participant_mute(store, provider, mutation).await
        }
        Err(error) => CommandExecution::transactional_failure(error),
    }
}
