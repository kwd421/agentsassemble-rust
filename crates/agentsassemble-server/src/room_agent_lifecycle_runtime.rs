use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartEffect, AgentStartPlan, AgentStopPlan, PersistenceError,
    SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderAdapterError, ProviderRuntimeStarted};

use crate::room_runtime::{CommandExecution, RoomCommand, progressed_execution};

pub(crate) async fn execute_agent_start(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let plan = if command.action == "agent.resume" {
        store
            .prepare_agent_resume(&command.principal, &command.request_id, &command.payload)
            .await
    } else {
        store
            .prepare_agent_start(&command.principal, &command.request_id, &command.payload)
            .await
    };
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
    match provider_adapter.start(&effect.session).await {
        Ok(started) => complete_agent_start(store, command, &effect, started).await,
        Err(error) => record_agent_start_failure(store, command, &effect, error).await,
    }
}

async fn complete_agent_start(
    store: &SqliteStore,
    command: &RoomCommand,
    effect: &AgentStartEffect,
    started: ProviderRuntimeStarted,
) -> CommandExecution {
    let persisted = persisted_start(started);
    let outcome = if command.action == "agent.resume" {
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
    let commit = if command.action == "agent.resume" {
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
    let plan = match store
        .prepare_agent_stop(&command.principal, &command.request_id, &command.payload)
        .await
    {
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
                Ok(outcome) => {
                    progressed_execution(store, &command.principal.room_id, outcome).await
                }
                Err(error) => CommandExecution::unresolved_failure(error),
            }
        }
        AgentStopPlan::Stop(effect) => {
            let stop = provider_adapter
                .stop(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.runtime_handle_id,
                    &effect.runtime_owner_id,
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
                )
                .await;
            match store
                .finalize_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await
            {
                Ok(outcome) => {
                    progressed_execution(store, &command.principal.room_id, outcome).await
                }
                Err(error) => CommandExecution::unresolved_failure(error),
            }
        }
    }
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
        provider_session_id: started.provider_session_id,
        runtime_reused: started.runtime_reused,
        provider_session_reused: started.provider_session_reused,
        provider_session_active: started.provider_session_active,
    }
}
