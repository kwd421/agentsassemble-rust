//! Non-confirmed delegated direct execution: lifecycle start/stop and atomic
//! mutations, their in-progress replies and durable publication.
use agentsassemble_domain::DirectRoomAction;
use agentsassemble_persistence::{DelegateRoomAuthority, PersistenceError, SqliteStore};
use serde_json::json;

use crate::principal_mutation_admission::PrincipalMutationAdmission;
use crate::room_command_execution::CommandExecution;
use crate::room_runtime::RoomCommandOwners;

use super::charge_request;

pub(crate) fn in_progress(request: &DirectRoomAction, operation_id: &str) -> serde_json::Value {
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
    use crate::room_agent_lifecycle_runtime::{
        LifecycleCommand, execute_agent_start, execute_agent_stop,
    };
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
