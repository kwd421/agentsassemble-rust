use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartEffect, AgentStartPlan, AgentStopPlan,
    LiveRuntimeReconciliation, PersistenceError, SqliteStore,
};
use agentsassemble_protocol::RoomAction;
use agentsassemble_provider::{ProviderAdapter, ProviderAdapterError, ProviderRuntimeStarted};

use crate::{
    room_runtime::{CommandExecution, RoomCommand, progressed_execution},
    runtime_reconciliation::recover_exact_lifecycle_command,
};

pub(crate) async fn execute_agent_start(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let mut plan = if command.action == RoomAction::AgentResume {
        store
            .prepare_agent_resume(&command.principal, &command.request_id, &command.payload)
            .await
    } else {
        store
            .prepare_agent_start(&command.principal, &command.request_id, &command.payload)
            .await
    };
    if plan.as_ref().is_err_and(unconfirmed_effect) {
        plan = match recover_exact_lifecycle_command(
            store,
            provider_adapter,
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await
        {
            Ok(LiveRuntimeReconciliation::RetryOriginalEffect) => {
                if command.action == RoomAction::AgentResume {
                    store
                        .prepare_agent_resume(
                            &command.principal,
                            &command.request_id,
                            &command.payload,
                        )
                        .await
                } else {
                    store
                        .prepare_agent_start(
                            &command.principal,
                            &command.request_id,
                            &command.payload,
                        )
                        .await
                }
            }
            Ok(LiveRuntimeReconciliation::StillUnresolved) => plan,
            Err(error) => Err(error),
        };
    }
    let plan = match plan {
        Ok(plan) => plan,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    let effect = match plan {
        AgentStartPlan::Outcome(outcome) => {
            return progressed_execution(store, &command.principal.room_id, *outcome).await;
        }
        AgentStartPlan::Start(effect) => effect,
    };
    let reservation = match provider_adapter.reserve_start(&effect.session).await {
        Ok(reservation) => reservation,
        Err(error) => {
            return record_agent_start_pre_effect_failure(store, command, &effect, error).await;
        }
    };
    let authorized = store
        .authorize_agent_start_effect(
            &command.principal,
            &command.request_id,
            &command.payload,
            &effect.operation_id,
            command.action.as_str(),
            &reservation.runtime_handle_id,
            &reservation.runtime_owner_id,
            &reservation.runtime_lease_token,
        )
        .await;
    let authorized = match authorized {
        Ok(effect) => effect,
        Err(error) => {
            provider_adapter
                .cancel_start_reservation(
                    &effect.session.public.room_id,
                    &effect.session.public.session_id,
                    &reservation,
                )
                .await;
            return CommandExecution::unresolved_failure(error);
        }
    };
    match provider_adapter.start_reserved(&authorized.session).await {
        Ok(started) => complete_agent_start(store, command, &authorized, started).await,
        Err(error) => {
            record_agent_start_failure(store, provider_adapter, command, &authorized, error).await
        }
    }
}

async fn record_agent_start_pre_effect_failure(
    store: &SqliteStore,
    command: &RoomCommand,
    effect: &AgentStartEffect,
    error: ProviderAdapterError,
) -> CommandExecution {
    let command_action = if command.action == RoomAction::AgentResume {
        "agent.resume"
    } else {
        "agent.start"
    };
    let commit = store
        .fail_agent_start_before_effect(
            &command.principal,
            &command.request_id,
            &command.payload,
            &effect.operation_id,
            error.code,
            error.message,
            command_action,
        )
        .await;
    let commit = match commit {
        Ok(commit) => commit,
        Err(recording_error) => return CommandExecution::unresolved_failure(recording_error),
    };
    CommandExecution::committed_failure(
        PersistenceError::StoredCommandRejected {
            code: commit.code,
            message: commit.message,
        },
        commit.events,
    )
}

async fn complete_agent_start(
    store: &SqliteStore,
    command: &RoomCommand,
    effect: &AgentStartEffect,
    started: ProviderRuntimeStarted,
) -> CommandExecution {
    let persisted = persisted_start(started);
    let outcome = if command.action == RoomAction::AgentResume {
        store
            .complete_agent_resume(
                &command.principal,
                &command.request_id,
                &command.payload,
                &effect.operation_id,
                &persisted,
            )
            .await
    } else {
        store
            .complete_agent_start(
                &command.principal,
                &command.request_id,
                &command.payload,
                &effect.operation_id,
                &persisted,
            )
            .await
    };
    match outcome {
        Ok(outcome) => progressed_execution(store, &command.principal.room_id, outcome).await,
        Err(error) => CommandExecution::unresolved_failure(error),
    }
}

async fn record_agent_start_failure(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
    effect: &AgentStartEffect,
    error: ProviderAdapterError,
) -> CommandExecution {
    if error.effect_uncertain {
        let events = match store
            .mark_agent_start_unconfirmed(
                &command.principal,
                &effect.session.public.session_id,
                &effect.operation_id,
                &error.runtime_handle_id,
                &error.runtime_owner_id,
                error.code,
                error.message,
            )
            .await
        {
            Ok(events) => events,
            Err(recording_error) => {
                return CommandExecution::unresolved_failure(recording_error);
            }
        };
        return CommandExecution::unresolved_failure_with_events(
            rejected(error.code, error.message),
            events,
        );
    }
    let commit = if command.action == RoomAction::AgentResume {
        store
            .fail_agent_resume(
                &command.principal,
                &command.request_id,
                &command.payload,
                &effect.operation_id,
                error.code,
                error.message,
            )
            .await
    } else {
        store
            .fail_agent_start(
                &command.principal,
                &command.request_id,
                &command.payload,
                &effect.operation_id,
                error.code,
                error.message,
            )
            .await
    };
    let commit = match commit {
        Ok(commit) => commit,
        Err(recording_error) => return CommandExecution::unresolved_failure(recording_error),
    };
    provider_adapter
        .release_checkpointed_start_absence(&effect.session)
        .await;
    CommandExecution::committed_failure(
        PersistenceError::StoredCommandRejected {
            code: commit.code,
            message: commit.message,
        },
        commit.events,
    )
}

pub(crate) async fn execute_agent_stop(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let plan = match prepare_agent_stop_with_recovery(store, provider_adapter, command).await {
        Ok(plan) => plan,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    match plan {
        AgentStopPlan::Outcome(outcome) => {
            progressed_execution(store, &command.principal.room_id, *outcome).await
        }
        AgentStopPlan::Finalize => {
            match store
                .finalize_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await
            {
                Ok(mutation) => CommandExecution::mutation(mutation),
                Err(error) => CommandExecution::unresolved_failure(error),
            }
        }
        AgentStopPlan::Stop(effect) => {
            let effect = match store
                .authorize_agent_stop_effect(
                    &command.principal,
                    &command.request_id,
                    &command.payload,
                    &effect.operation_id,
                )
                .await
            {
                Ok(effect) => effect,
                Err(error) => return CommandExecution::unresolved_failure(error),
            };
            let stop = provider_adapter
                .stop(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.runtime_handle_id,
                    &effect.runtime_owner_id,
                    &effect.runtime_lease_token,
                )
                .await;
            if let Err(error) = stop {
                let events = match store
                    .mark_agent_stop_unconfirmed(
                        &command.principal,
                        &effect.session_id,
                        &effect.operation_id,
                        error.code,
                        error.message,
                    )
                    .await
                {
                    Ok(events) => events,
                    Err(recording_error) => {
                        return CommandExecution::unresolved_failure(recording_error);
                    }
                };
                return CommandExecution::unresolved_failure_with_events(
                    PersistenceError::CommandRejected {
                        code: error.code,
                        message: error.message.to_owned(),
                    },
                    events,
                );
            }
            if let Err(error) = store
                .record_agent_stop_effect(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.operation_id,
                )
                .await
            {
                return CommandExecution::unresolved_failure(error);
            }
            provider_adapter
                .release_confirmed_stop(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.runtime_handle_id,
                    &effect.runtime_owner_id,
                    &effect.runtime_lease_token,
                )
                .await;
            match store
                .finalize_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await
            {
                Ok(mutation) => CommandExecution::mutation(mutation),
                Err(error) => CommandExecution::unresolved_failure(error),
            }
        }
    }
}

async fn prepare_agent_stop_with_recovery(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> Result<AgentStopPlan, PersistenceError> {
    let plan = store
        .prepare_agent_stop(&command.principal, &command.request_id, &command.payload)
        .await;
    if !plan.as_ref().is_err_and(unconfirmed_effect) {
        return plan;
    }
    match recover_exact_lifecycle_command(
        store,
        provider_adapter,
        &command.principal,
        &command.request_id,
        command.action.as_str(),
        &command.payload,
    )
    .await
    {
        Ok(LiveRuntimeReconciliation::RetryOriginalEffect) => {
            store
                .prepare_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await
        }
        Ok(LiveRuntimeReconciliation::StillUnresolved) => plan,
        Err(error) => Err(error),
    }
}

fn unconfirmed_effect(error: &PersistenceError) -> bool {
    matches!(
        error,
        PersistenceError::CommandUnresolved {
            code: "runtime_effect_unconfirmed",
            ..
        }
    )
}

fn rejected(code: &'static str, message: &str) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.to_owned(),
    }
}

fn persisted_start(started: ProviderRuntimeStarted) -> AgentRuntimeStarted {
    AgentRuntimeStarted {
        runtime_handle_id: started.runtime_handle_id,
        runtime_owner_id: started.runtime_owner_id,
        runtime_lease_token: started.runtime_lease_token,
        provider_session_id: started.provider_session_id,
        runtime_reused: started.runtime_reused,
        provider_session_reused: started.provider_session_reused,
        provider_session_active: started.provider_session_active,
    }
}
