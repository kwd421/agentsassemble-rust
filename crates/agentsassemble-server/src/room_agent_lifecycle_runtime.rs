use agentsassemble_persistence::{
    AgentRuntimeStarted, AgentStartPlan, AgentStopPlan, PersistenceError, SqliteStore,
};
use agentsassemble_provider::{ProviderAdapter, ProviderRuntimeStarted};

use crate::room_runtime::{CommandExecution, RoomCommand, progressed_execution};

pub(crate) async fn execute_agent_start(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> Result<CommandExecution, PersistenceError> {
    let plan = if command.action == "agent.resume" {
        store
            .prepare_agent_resume(&command.principal, &command.request_id, &command.payload)
            .await?
    } else {
        store
            .prepare_agent_start(&command.principal, &command.request_id, &command.payload)
            .await?
    };
    let effect = match plan {
        AgentStartPlan::Outcome(outcome) => {
            return Ok(progressed_execution(store, &command.principal.room_id, *outcome).await);
        }
        AgentStartPlan::Start(effect) => effect,
    };
    match provider_adapter.start(&effect.session).await {
        Ok(started) => {
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
                    .await?
            } else {
                store
                    .complete_agent_start(
                        &command.principal,
                        &command.request_id,
                        &command.payload,
                        &effect.operation_id,
                        &persisted,
                    )
                    .await?
            };
            Ok(progressed_execution(store, &command.principal.room_id, outcome).await)
        }
        Err(error) => {
            let events = if error.effect_uncertain {
                store
                    .mark_agent_start_unconfirmed(
                        &command.principal,
                        &effect.session.public.session_id,
                        &effect.operation_id,
                        &error.runtime_handle_id,
                        &error.runtime_owner_id,
                        error.code,
                        error.message,
                    )
                    .await?
            } else if command.action == "agent.resume" {
                store
                    .fail_agent_resume(
                        &command.principal,
                        &command.request_id,
                        &command.payload,
                        &effect.operation_id,
                        error.code,
                        error.message,
                    )
                    .await?
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
                    .await?
            };
            Ok(CommandExecution::committed_failure(
                PersistenceError::CommandRejected {
                    code: error.code,
                    message: error.message.to_owned(),
                },
                events,
            ))
        }
    }
}

pub(crate) async fn execute_agent_stop(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> Result<CommandExecution, PersistenceError> {
    match store
        .prepare_agent_stop(&command.principal, &command.request_id, &command.payload)
        .await?
    {
        AgentStopPlan::Outcome(outcome) => {
            Ok(progressed_execution(store, &command.principal.room_id, *outcome).await)
        }
        AgentStopPlan::Finalize => {
            let outcome = store
                .finalize_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await?;
            Ok(progressed_execution(store, &command.principal.room_id, outcome).await)
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
                let events = store
                    .mark_agent_stop_unconfirmed(
                        &command.principal,
                        &effect.session_id,
                        &effect.operation_id,
                        error.code,
                        error.message,
                    )
                    .await?;
                return Ok(CommandExecution::committed_failure(
                    PersistenceError::CommandRejected {
                        code: error.code,
                        message: error.message.to_owned(),
                    },
                    events,
                ));
            }
            store
                .record_agent_stop_effect(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.operation_id,
                )
                .await?;
            provider_adapter
                .release_confirmed_stop(
                    &command.principal.room_id,
                    &effect.session_id,
                    &effect.runtime_handle_id,
                    &effect.runtime_owner_id,
                )
                .await;
            let outcome = store
                .finalize_agent_stop(&command.principal, &command.request_id, &command.payload)
                .await?;
            Ok(progressed_execution(store, &command.principal.room_id, outcome).await)
        }
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
