use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{
    AgentInterruptPlan, PersistenceError, ProviderTurnInterruptEffect, SqliteStore,
};
use agentsassemble_protocol::RoomAction;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService};
use tokio::sync::broadcast;

use crate::{
    agent_create_runtime::AgentCreateExecution,
    room_command_execution::{
        CommandExecution, persistence_error_code, progress_execution, progressed_execution,
    },
    room_runtime::{RoomCommand, RoomCommandSession},
};

pub(crate) async fn execute_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> CommandExecution {
    if let Some(RoomCommandSession::Browser(authorization)) = &command.session
        && requires_session_dispatch(authorization, command.action)
    {
        return execute_room_session_command(store, command, authorization).await;
    }
    if let Some(RoomCommandSession::Connector(authorization)) = &command.session {
        return crate::room_runtime::connector::execute(store, command, authorization).await;
    }
    let authority = command.mutation_authority();
    match command.action {
        RoomAction::RoomDelete => execute_room_delete(store, command).await,
        RoomAction::RoomClose | RoomAction::RoomArchive => {
            execute_room_lifecycle(store, command).await
        }
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
        RoomAction::AgentProfileUpdate
        | RoomAction::ChannelMessageSend
        | RoomAction::ParticipantRoleUpdate => execute_atomic_update(store, command).await,
        RoomAction::AgentPause => {
            crate::room_agent_lifecycle_runtime::execute_agent_pause(
                store,
                provider_adapter,
                command,
            )
            .await
        }
        RoomAction::AgentInterrupt => {
            execute_agent_interrupt(store, provider_adapter, command).await
        }
        RoomAction::AgentStart | RoomAction::AgentResume | RoomAction::AgentReadd => {
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
            .execute_room_settings_update(authority, &command.request_id, &command.payload)
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
        RoomAction::ParticipantMute => {
            execute_participant_mute(store, provider_adapter, command).await
        }
        RoomAction::ParticipantLeave => execute_participant_leave(store, command).await,
        RoomAction::ParticipantKick | RoomAction::ParticipantExport => {
            execute_participant_removal(store, provider_adapter, command).await
        }
        RoomAction::RoomHistory
        | RoomAction::ChannelHistory
        | RoomAction::RoomVoteSummary
        | RoomAction::SideChatSend
        | RoomAction::ProviderRequestResolve => misrouted_durable_command(command.action),
    }
}

// These commands own one authority transaction and publication, with no additional
// provider effect or scheduler transition to reconcile after the commit.
async fn execute_atomic_update(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    let authority = command.mutation_authority();
    let result = match command.action {
        RoomAction::AgentProfileUpdate => {
            store
                .execute_agent_profile_update(authority, &command.request_id, &command.payload)
                .await
        }
        RoomAction::ChannelMessageSend => {
            store
                .execute_channel_message(authority, &command.request_id, &command.payload)
                .await
        }
        RoomAction::ParticipantRoleUpdate => {
            store
                .execute_participant_role_update(authority, &command.request_id, &command.payload)
                .await
        }
        _ => return misrouted_durable_command(command.action),
    };
    result.map_or_else(
        CommandExecution::transactional_failure,
        CommandExecution::success,
    )
}

async fn execute_room_delete(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    match store
        .execute_room_delete(
            command.mutation_authority(),
            &command.request_id,
            &command.payload,
        )
        .await
    {
        Ok(mutation) => {
            let mut execution = if mutation.complete {
                CommandExecution::success(mutation.outcome)
            } else {
                CommandExecution::unresolved_failure_with_events(PersistenceError::CommandUnresolved {
                        code: "room_deletion_pending", message: "The room is closed. Deletion is waiting for owned runtime cleanup and publication; retry the same request.".to_owned(),
                    }, if mutation.outcome.deduplicated { Vec::new() } else { mutation.outcome.events })
            };
            execution.revoked_human_sessions = mutation.revoked_session_fingerprints;
            execution
        }
        Err(error) => CommandExecution::transactional_failure(error),
    }
}

async fn execute_room_lifecycle(store: &SqliteStore, command: &RoomCommand) -> CommandExecution {
    match store
        .execute_room_lifecycle(
            command.mutation_authority(),
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await
    {
        Ok(mutation) => {
            let mut execution = CommandExecution::success(mutation.outcome);
            execution.revoked_human_sessions = mutation.revoked_session_fingerprints;
            // The existing watcher cleans durable pending runtime custody; live
            // human revocation and HTTP ACK must not wait for every provider.
            execution
        }
        Err(error) => CommandExecution::transactional_failure(error),
    }
}

async fn execute_participant_removal(
    store: &SqliteStore,
    adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let mutation = match store
        .execute_participant_removal(
            command.mutation_authority(),
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await
    {
        Ok(mutation) => mutation,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    let mut execution = CommandExecution::success(mutation.outcome);
    execution.revoked_human_sessions = mutation.revoked_session_fingerprints;
    if let Some(key) = mutation.cleanup {
        match Box::pin(crate::room_runtime_cleanup::attempt_cleanup(
            store, adapter, &key,
        ))
        .await
        {
            Ok(Some(commit)) => execution.extend_turn_commit(commit),
            Ok(None) => {}
            Err(error) => crate::room_runtime_cleanup::log_pending(&key, &error),
        }
    }
    execution
}

async fn execute_participant_mute(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let mutation = match store
        .execute_participant_mute(
            command.mutation_authority(),
            &command.request_id,
            &command.payload,
        )
        .await
    {
        Ok(mutation) => mutation,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    let effect = mutation.host_interrupt_effect.clone();
    let mut execution = CommandExecution::participant_mute(mutation);
    if let Some(effect) = effect {
        match Box::pin(
            crate::provider_turn_interrupt_runtime::apply_exact_interrupt(
                store,
                provider_adapter,
                &effect,
            ),
        )
        .await
        {
            Ok(commit) => execution.extend_turn_commit(commit),
            Err(error) => log_interrupt_error(&error, command, &effect),
        }
    }
    execution
}

async fn execute_agent_interrupt(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> CommandExecution {
    let plan = match store
        .prepare_agent_interrupt(
            command.mutation_authority(),
            &command.request_id,
            &command.payload,
        )
        .await
    {
        Ok(plan) => plan,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    let managed = match plan {
        AgentInterruptPlan::External => None,
        AgentInterruptPlan::Outcome(outcome) => return CommandExecution::success(*outcome),
        AgentInterruptPlan::Interruptible {
            session,
            durable_turn_is_assigned,
        } => Some((session, durable_turn_is_assigned)),
    };
    if let Some((session, durable_turn_is_assigned)) = managed
        && let Err(error) = provider_adapter
            .require_retained_turn_interrupt(&session, durable_turn_is_assigned)
            .await
    {
        return CommandExecution::transactional_failure(PersistenceError::CommandRejected {
            code: error.code,
            message: error.message.to_owned(),
        });
    }
    let mutation = match store
        .execute_agent_interrupt(
            command.mutation_authority(),
            &command.request_id,
            &command.payload,
        )
        .await
    {
        Ok(mutation) => mutation,
        Err(error) => return CommandExecution::transactional_failure(error),
    };
    let effect = mutation.host_interrupt_effect;
    let mut execution = CommandExecution::success(mutation.outcome);
    if let Some(effect) = effect {
        match Box::pin(
            crate::provider_turn_interrupt_runtime::apply_exact_interrupt(
                store,
                provider_adapter,
                &effect,
            ),
        )
        .await
        {
            Ok(commit) => execution.extend_turn_commit(commit),
            Err(error) => log_agent_interrupt_error(&error, command, &effect),
        }
    }
    execution
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

fn misrouted_durable_command(action: RoomAction) -> CommandExecution {
    CommandExecution::transactional_failure(PersistenceError::CommandRejected {
        code: "command_action_misrouted",
        message: format!(
            "{} cannot enter the durable room mutation owner.",
            action.as_str()
        ),
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

fn log_agent_interrupt_error(
    error: &PersistenceError,
    command: &RoomCommand,
    effect: &ProviderTurnInterruptEffect,
) {
    tracing::error!(
        code = persistence_error_code(error),
        room_id = command.principal.room_id,
        session_id = effect.session_id,
        "explicit provider interrupt was accepted; exact effect remains owned by recovery"
    );
}

fn requires_session_dispatch(
    authorization: &agentsassemble_persistence::RoomSessionAuthorization,
    action: RoomAction,
) -> bool {
    if action == RoomAction::ChannelMessageSend {
        // This owner resolves native, paired and human authority in its transaction.
        return false;
    }
    matches!(
        authorization,
        agentsassemble_persistence::RoomSessionAuthorization::Human(_)
    ) || matches!(
        action,
        RoomAction::MessageSend
            | RoomAction::MessageEdit
            | RoomAction::MessageDelete
            | RoomAction::RoomRandomRoll
            | RoomAction::RoomRandomChoose
            | RoomAction::ParticipantLeave
    )
}

async fn execute_room_session_command(
    store: &SqliteStore,
    command: &RoomCommand,
    authorization: &agentsassemble_persistence::RoomSessionAuthorization,
) -> CommandExecution {
    match command.action {
        RoomAction::MessageSend => match store
            .execute_authorized_message_with_turn(
                authorization.mutation_authority(),
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
            .execute_room_session_message_mutation(
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
            match crate::room_random_runtime::execute_authorized_room_random(
                store,
                command,
                authorization.mutation_authority(),
            )
            .await
            {
                Ok(outcome) => CommandExecution::success(outcome),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        RoomAction::ParticipantLeave => match store
            .execute_room_session_participant_leave(
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
        command,
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
            command.mutation_authority(),
            &command.request_id,
            command.action.as_str(),
            &command.payload,
        )
        .await?
    {
        return Ok(CommandExecution::success(outcome));
    }
    let current = store
        .agent_configuration_candidate(command.mutation_authority(), &command.payload)
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
            command.mutation_authority(),
            &command.request_id,
            &command.payload,
            &expected_profile_key,
            &selection.into(),
        )
        .await
        .map(CommandExecution::success)
}
