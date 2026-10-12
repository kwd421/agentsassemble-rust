use agentsassemble_domain::{DirectRoomAction, canonical_payload_hash};
use agentsassemble_persistence::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use serde_json::json;
use tokio::sync::oneshot;

use super::{RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};
use crate::room_command_execution::CommandExecution;

pub(super) struct DelegatedCommand {
    authority: DelegateRoomAuthority,
    request: DirectRoomAction,
    admission: PrincipalMutationAdmission,
    reply: oneshot::Sender<Result<serde_json::Value, PersistenceError>>,
}

impl RoomRuntime {
    /// Executes an action on the room actor, retaining moderation effects and floor progression.
    /// # Errors
    /// Rejects ended custody, budget exhaustion, conflicting retries and invalid targets.
    pub async fn execute_delegated_direct(
        &self,
        authority: &DelegateRoomAuthority,
        request: &DirectRoomAction,
    ) -> Result<serde_json::Value, PersistenceError> {
        let handle = self.handle(authority.room_id()).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Delegated(DelegatedCommand {
                authority: authority.clone(),
                request: request.clone(),
                admission: self.principal_mutations.clone(),
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
    execute_with_reply(
        owners,
        &command.admission,
        &command.authority,
        &command.request,
        |result| {
            let _ = command.reply.send(result);
        },
    )
    .await
}

pub(crate) async fn execute_with_reply(
    owners: RoomCommandOwners<'_>,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    reply: impl FnOnce(Result<serde_json::Value, PersistenceError>),
) -> Option<crate::event_publication::PublicationAttempt> {
    let (reserved, mut progress) = tokio::sync::mpsc::unbounded_channel();
    let execution = apply(
        owners.store,
        owners.provider_adapter,
        admission,
        authority,
        request,
        owners.lifecycle_commands,
        Some(&reserved),
    );
    tokio::pin!(execution);
    let mut reply = Some(reply);
    let execution = tokio::select! {
        result = &mut execution => result,
        Some(operation_id) = progress.recv() => {
            if let Some(reply) = reply.take() {
                reply(Ok(in_progress(request, &operation_id)));
            }
            execution.await
        }
    };
    let publication = publish(owners, &execution.execution).await;
    if let Some(reply) = reply {
        reply(execution.into_result());
    }
    publication
}

fn in_progress(request: &DirectRoomAction, operation_id: &str) -> serde_json::Value {
    json!({"status":"in_progress","request_id":request.request_id(),"action":request.name(),
        "target_id":json!(request)["agent_id"],"operation_id":operation_id})
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

pub(crate) struct DirectExecution {
    pub(crate) execution: CommandExecution,
    pending: Option<serde_json::Value>,
}
impl DirectExecution {
    pub(crate) fn into_result(self) -> Result<serde_json::Value, PersistenceError> {
        if let Some(pending) = self.pending {
            return Ok(pending);
        }
        self.execution
            .reply
            .map(|outcome| outcome.result)
            .map_err(|failure| failure.error)
    }
}
impl From<CommandExecution> for DirectExecution {
    fn from(execution: CommandExecution) -> Self {
        Self {
            execution,
            pending: None,
        }
    }
}

pub(crate) async fn apply(
    store: &SqliteStore,
    provider: &agentsassemble_provider::ProviderAdapter,
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    tracker: &crate::lifecycle_command_tracker::LifecycleCommandTracker,
    reserved: Option<&tokio::sync::mpsc::UnboundedSender<String>>,
) -> DirectExecution {
    let _inflight = match admission.acquire_inflight() {
        Ok(permit) => permit,
        Err(error) => return CommandExecution::transactional_failure(error).into(),
    };
    let charge = |owner: &str, delegate: &str, bytes| {
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
    };
    if matches!(
        request,
        DirectRoomAction::StartAgent(_) | DirectRoomAction::StopAgent(_)
    ) {
        let (control, debits) = match store
            .admit_delegated_agent_control(authority, request, charge)
            .await
        {
            Ok(admitted) => admitted,
            Err(error) => return CommandExecution::transactional_failure(error).into(),
        };
        let Some(_guard) = tracker.try_claim(
            authority.room_id(),
            &control.principal().principal_id,
            control.request_id(),
            control.action(),
        ) else {
            return CommandExecution::unresolved_failure(PersistenceError::CommandUnresolved {
                code: "runtime_recovery_in_progress".into(),
                message:
                    "The exact lifecycle request is owned by recovery; retry the same request."
                        .into(),
            })
            .into();
        };
        use crate::room_agent_lifecycle_runtime::{
            LifecycleCommand, execute_agent_start, execute_agent_stop,
        };
        let action = if matches!(request, DirectRoomAction::StartAgent(_)) {
            agentsassemble_protocol::RoomAction::AgentStart
        } else {
            agentsassemble_protocol::RoomAction::AgentStop
        };
        let command = LifecycleCommand::delegated(&control, action, reserved);
        let mut execution = if action == agentsassemble_protocol::RoomAction::AgentStart {
            execute_agent_start(store, provider, &command).await
        } else {
            execute_agent_stop(store, provider, &command).await
        };
        if execution.is_definitive()
            && let Some(debits) = debits
        {
            for debit in debits {
                debit.resolve();
            }
        }
        execution.reply = execution.reply.and_then(|outcome| {
            crate::room_command_result::public_command_outcome(control.principal(), outcome)
                .map_err(crate::room_command_result::CommandFailure::unresolved)
        });
        let pending = execution
            .reply
            .as_ref()
            .err()
            .filter(|failure| {
                matches!(&failure.error,
            PersistenceError::CommandUnresolved {code,..} if code == "external_stop_pending")
            })
            .map(|_| in_progress(request, &control.operation_id()));
        return DirectExecution { execution, pending };
    }
    match store
        .execute_delegated_atomic(authority, request, charge)
        .await
    {
        Ok((mutation, debits)) => {
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            crate::room_command_dispatch::finish_participant_mute(store, provider, mutation)
                .await
                .into()
        }
        Err(error) => CommandExecution::transactional_failure(error).into(),
    }
}
