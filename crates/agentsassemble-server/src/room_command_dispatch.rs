use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{PersistenceError, ProviderTurnInterruptEffect, SqliteStore};
use agentsassemble_protocol::RoomAction;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use tokio::sync::broadcast;

use crate::{
    agent_create_runtime::AgentCreateExecution,
    room_command_execution::{
        CommandExecution, persistence_error_code, progress_execution, progressed_execution,
    },
    room_runtime::RoomCommand,
};

pub(crate) async fn execute_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> CommandExecution {
    if let Some(authorization) = &command.human_session {
        return execute_human_session_command(store, command, authorization).await;
    }
    match command.action {
        RoomAction::AgentCreate => {
            execute_agent_create_command(
                store,
                provider_catalog,
                provider_adapter,
                event_tx,
                command,
            )
            .await
        }
        RoomAction::AgentConfigure => execute_agent_configure(store, provider_catalog, command)
            .await
            .unwrap_or_else(CommandExecution::transactional_failure),
        RoomAction::AgentPause => {
            crate::room_agent_lifecycle_runtime::execute_agent_pause(store, command).await
        }
        RoomAction::AgentStart | RoomAction::AgentResume => {
            crate::room_agent_lifecycle_runtime::execute_agent_start(
                store,
                provider_adapter,
                command,
            )
            .await
        }
        RoomAction::AgentStop => {
            crate::room_agent_lifecycle_runtime::execute_agent_stop(
                store,
                provider_adapter,
                command,
            )
            .await
        }
        RoomAction::RoomSettingsUpdate => match store
            .execute_room_settings_update(&command.principal, &command.request_id, &command.payload)
            .await
        {
            Ok(outcome) if outcome.deduplicated => CommandExecution::success(outcome),
            Ok(outcome) => progressed_execution(store, &command.principal.room_id, outcome).await,
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::RoomRandomRoll | RoomAction::RoomRandomChoose => {
            match crate::room_random_runtime::execute_room_random(store, command).await {
                Ok(outcome) => CommandExecution::success(outcome),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        RoomAction::MessageSend => execute_message_send(store, command).await,
        RoomAction::MessageEdit | RoomAction::MessageDelete => {
            execute_message_mutation(store, command).await
        }
        RoomAction::ParticipantRoleUpdate => match store
            .execute_participant_role_update(
                &command.principal,
                &command.request_id,
                &command.payload,
            )
            .await
        {
            Ok(outcome) => CommandExecution::success(outcome),
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::ParticipantMute => match store
            .execute_participant_mute(&command.principal, &command.request_id, &command.payload)
            .await
        {
            Ok(mutation) => {
                let effect = mutation.interrupt_effect.clone();
                let mut execution = CommandExecution::participant_mute(mutation);
                if let Some(effect) = effect {
                    match Box::pin(crate::participant_mute_runtime::apply_exact_interrupt(
                        store,
                        provider_adapter,
                        &effect,
                    ))
                    .await
                    {
                        Ok(commit) => execution.extend_turn_commit(commit),
                        Err(error) => log_interrupt_error(&error, command, &effect),
                    }
                }
                execution
            }
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::ParticipantLeave => execute_participant_leave(store, command).await,
        RoomAction::RoomHistory | RoomAction::RoomVoteSummary => {
            misrouted_direct_read(command.action)
        }
    }
}

async fn execute_message_send(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    match store
        .execute_message_with_turn(
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await
    {
        Ok(mutation) => CommandExecution::mutation(mutation),
        Err(error) => CommandExecution::transactional_failure(error),
    }
}

async fn execute_message_mutation(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    match store
        .execute_message_mutation(
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await
    {
        Ok(outcome) => CommandExecution::success(outcome),
        Err(error) => CommandExecution::transactional_failure(error),
    }
}

fn misrouted_direct_read(action: RoomAction) -> CommandExecution {
    CommandExecution::transactional_failure(PersistenceError::CommandRejected {
        code: "read_action_misrouted",
        message: format!("{} cannot enter the room mutation owner.", action.as_str()),
    })
}

async fn execute_participant_leave(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    match store
        .execute_participant_leave(&command.principal, &command.request_id, &command.payload)
        .await
    {
        Ok(mutation) => CommandExecution::participant_leave(mutation),
        Err(error) => CommandExecution::transactional_failure(error),
    }
}

fn log_interrupt_error(
    error: &PersistenceError,
    command: &RoomCommand,
    effect: &ProviderTurnInterruptEffect,
) {
    tracing::error!(
        code = persistence_error_code(error),
        room_id = command.principal.room_id,
        session_id = effect.session_id,
        "participant mute committed; exact provider interrupt remains quarantined"
    );
}

async fn execute_human_session_command(
    store: &SqliteStore,
    command: &RoomCommand,
    authorization: &agentsassemble_persistence::HumanSessionAuthorization,
) -> CommandExecution {
    match command.action {
        RoomAction::MessageSend => match store
            .execute_human_session_message_with_turn(
                authorization,
                &command.request_id,
                command.action.as_str(),
                &command.payload,
            )
            .await
        {
            Ok(mutation) => CommandExecution::mutation(mutation),
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::MessageEdit | RoomAction::MessageDelete => match store
            .execute_human_session_message_mutation(
                authorization,
                &command.request_id,
                command.action.as_str(),
                &command.payload,
            )
            .await
        {
            Ok(outcome) => CommandExecution::success(outcome),
            Err(error) => CommandExecution::transactional_failure(error),
        },
        RoomAction::RoomRandomRoll | RoomAction::RoomRandomChoose => {
            match crate::room_random_runtime::execute_human_session_room_random(
                store,
                command,
                authorization,
            )
            .await
            {
                Ok(outcome) => CommandExecution::success(outcome),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        RoomAction::ParticipantLeave => match store
            .execute_human_session_participant_leave(
                authorization,
                &command.request_id,
                &command.payload,
            )
            .await
        {
            Ok(mutation) => CommandExecution::participant_leave(mutation),
            Err(error) => CommandExecution::transactional_failure(error),
        },
        _ => CommandExecution::transactional_failure(PersistenceError::CommandRejected {
            code: "permission_denied",
            message: "This human room session cannot perform that action.".to_owned(),
        }),
    }
}

async fn execute_agent_create_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> CommandExecution {
    let AgentCreateExecution {
        reply,
        committed_events,
        advance_ordered_floor,
    } = match crate::agent_create_runtime::execute_agent_create(
        store,
        provider_catalog,
        provider_adapter,
        event_tx,
        &command.principal,
        &command.request_id,
        &command.payload,
    )
    .await
    {
        Ok(execution) => execution,
        Err(failure) => return CommandExecution::failure(failure),
    };
    let execution = CommandExecution {
        reply,
        committed_events,
        assignments: Vec::new(),
        revoked_human_sessions: Vec::new(),
    };
    if advance_ordered_floor {
        progress_execution(store, &command.principal.room_id, execution).await
    } else {
        execution
    }
}

async fn execute_agent_configure(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    command: &RoomCommand,
) -> Result<CommandExecution, PersistenceError> {
    if let Some(outcome) = store
        .replay_command(
            &command.principal,
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await?
    {
        return Ok(CommandExecution::success(outcome));
    }
    let current = store
        .agent_configuration_candidate(&command.principal, &command.payload)
        .await?;
    let expected_profile_key = current.runtime_profile_key.clone();
    let selection = provider_catalog
        .validate_configuration(
            &command.principal.room_id,
            &command.principal.principal_id,
            &command.request_id,
            &current,
            &command.payload,
        )
        .await
        .map_err(|error| PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        })?;
    store
        .execute_agent_configuration(
            &command.principal,
            &command.request_id,
            &command.payload,
            &expected_profile_key,
            &selection.into(),
        )
        .await
        .map(CommandExecution::success)
}
