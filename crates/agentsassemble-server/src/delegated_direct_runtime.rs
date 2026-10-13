use agentsassemble_domain::{DirectRoomAction, canonical_payload_hash};
use agentsassemble_persistence::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use serde_json::json;
use tokio::sync::oneshot;

use super::{RoomCommand, RoomCommandOwners, RoomMutation, RoomRuntime};
use crate::lifecycle_command_tracker::LifecycleCommandTracker;
use crate::principal_mutation_admission::{MutationIdentity, PrincipalMutationAdmission};
use crate::room_agent_lifecycle_runtime::{LifecycleCommand, execute_agent_start};
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
    if let DirectRoomAction::RequestDeleteTextChannel(delete) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let (result, debits) = owners
                .store
                .request_delegated_channel_delete(authority, delete, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            if let Some(debits) = debits {
                for debit in debits {
                    debit.resolve();
                }
            }
            Ok(result)
        }
        .await;
        let publication = if result.is_ok() {
            Some(
                crate::event_publication::publish_durable_room_events(
                    owners.store,
                    owners.event_tx,
                    authority.room_id(),
                )
                .await,
            )
        } else {
            None
        };
        reply(result);
        return publication;
    }
    if let DirectRoomAction::StartAgent(start) = request {
        let result = async {
            let _inflight = admission.acquire_inflight()?;
            let pending = owners
                .store
                .request_delegated_agent_start(authority, start, |owner, delegate, bytes| {
                    charge_request(admission, authority, request, owner, delegate, bytes)
                })
                .await?;
            match pending {
                Some((result, debits)) => {
                    if let Some(debits) = debits {
                        for debit in debits {
                            debit.resolve();
                        }
                    }
                    Ok(Some(result))
                }
                None => Ok(None),
            }
        }
        .await;
        match result {
            Ok(Some(result)) => {
                let publication = Some(
                    crate::event_publication::publish_durable_room_events(
                        owners.store,
                        owners.event_tx,
                        authority.room_id(),
                    )
                    .await,
                );
                reply(Ok(result));
                return publication;
            }
            Ok(None) => {}
            Err(error) => {
                reply(Err(error));
                return None;
            }
        }
    }
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
        charge_request(admission, authority, request, owner, delegate, bytes)
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

fn charge_request(
    admission: &PrincipalMutationAdmission,
    authority: &DelegateRoomAuthority,
    request: &DirectRoomAction,
    owner: &str,
    delegate: &str,
    bytes: usize,
) -> Result<[crate::principal_mutation_admission::MutationDebit; 2], PersistenceError> {
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
}

/// An approved confirmed agent start reserved its lifecycle operation inside the
/// decision transaction; drive it through the same gate the owner's own start uses.
/// The owner's ACK still reports the durable in-progress receipt; completion,
/// failure and recovery travel through the existing lifecycle custody.
pub(crate) async fn drive_approved_confirmed_start(
    store: &SqliteStore,
    provider_adapter: &agentsassemble_provider::ProviderAdapter,
    lifecycle_commands: &LifecycleCommandTracker,
    command: &RoomCommand,
    execution: &mut CommandExecution,
) {
    if command.action != agentsassemble_protocol::RoomAction::DelegatedActionDecide {
        return;
    }
    let Ok(outcome) = &execution.reply else {
        return;
    };
    if outcome.deduplicated
        || outcome
            .result
            .get("state")
            .and_then(serde_json::Value::as_str)
            != Some("executing")
        || outcome
            .result
            .get("receipt")
            .and_then(|receipt| receipt.get("action"))
            .and_then(serde_json::Value::as_str)
            != Some("start_agent")
    {
        return;
    }
    let Some(confirmation_id) = outcome
        .result
        .get("confirmation_id")
        .and_then(serde_json::Value::as_str)
    else {
        return;
    };
    let confirmation_id = confirmation_id.to_owned();
    let control = match store
        .confirmed_agent_start(&command.principal.room_id, &confirmation_id)
        .await
    {
        Ok(control) => control,
        Err(error) => {
            tracing::error!(error = ?error, room_id = %command.principal.room_id, "approved confirmed start could not load its reserved control");
            return;
        }
    };
    let Some(_guard) = lifecycle_commands.try_claim(
        &control.principal().room_id,
        &control.principal().principal_id,
        control.request_id(),
        "agent.start",
    ) else {
        // Server recovery owns the reserved command; it reconciles the effect.
        return;
    };
    let lifecycle = LifecycleCommand::delegated(
        &control,
        agentsassemble_protocol::RoomAction::AgentStart,
        None,
    );
    let driven = execute_agent_start(store, provider_adapter, &lifecycle).await;
    execution.committed_events.extend(driven.committed_events);
    execution.assignments.extend(driven.assignments);
    if let Err(failure) = driven.reply {
        // Terminal failures are already recorded in the confirmation receipt;
        // uncertain effects stay executing under lifecycle reconciliation.
        tracing::warn!(error = ?failure.error, room_id = %command.principal.room_id, confirmation_id, "approved confirmed start did not complete");
    }
}
