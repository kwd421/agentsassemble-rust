use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{
    AgentCreateStartEffect, AgentCreateStartPlan, CommandOutcome, LiveRuntimeReconciliation,
    PersistenceError, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAdapterError, ProviderCatalogService, ProviderRuntimeStarted,
    creation_start_requested,
};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    room_agent_lifecycle_runtime::persisted_start,
    room_command_result::{CommandFailure, ended_session_authority},
    room_runtime::RoomCommand,
    runtime_reconciliation::recover_exact_lifecycle_command,
};

pub(crate) struct AgentCreateExecution {
    pub reply: Result<CommandOutcome, CommandFailure>,
    pub committed_events: Vec<RoomEvent>,
    pub advance_ordered_floor: bool,
}

pub(crate) async fn execute_agent_create(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> Result<AgentCreateExecution, CommandFailure> {
    let principal = &command.principal;
    let request_id = command.request_id.as_str();
    let payload = &command.payload;
    let start_requested = creation_start_requested(payload)
        .map_err(|error| CommandFailure::rejected(selection_error(error)))?;
    if start_requested {
        return execute_agent_create_start(
            store,
            provider_catalog,
            provider_adapter,
            event_tx,
            command,
        )
        .await;
    }
    if let Some(outcome) = store
        .replay_command(
            command.mutation_authority(),
            request_id,
            "agent.create",
            payload,
        )
        .await
        .map_err(CommandFailure::transactional)?
    {
        return Ok(success(outcome, false));
    }
    let selection = provider_catalog
        .validate_creation(
            &principal.room_id,
            &principal.principal_id,
            request_id,
            payload,
        )
        .await
        .map_err(|error| CommandFailure::rejected(selection_error(error)))?;
    let outcome = store
        .execute_agent_create(
            command.mutation_authority(),
            request_id,
            payload,
            &selection.into(),
        )
        .await
        .map_err(CommandFailure::transactional)?;
    Ok(success(outcome, false))
}

async fn execute_agent_create_start(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> Result<AgentCreateExecution, CommandFailure> {
    let principal = &command.principal;
    let request_id = command.request_id.as_str();
    let payload = &command.payload;
    let plan = resolve_create_start_plan(store, provider_catalog, adapter, command).await?;
    let effect = match plan {
        AgentCreateStartPlan::Outcome(outcome) => return Ok(success(*outcome, false)),
        AgentCreateStartPlan::Start(effect) => effect,
        AgentCreateStartPlan::Select => {
            return Err(CommandFailure::unresolved(rejected(
                "invalid_state",
                "Create/start selection did not produce a durable intent.",
            )));
        }
    };
    if !effect.newly_committed_events.is_empty()
        && let Err(error) =
            crate::event_publication::drain_room_publications(store, event_tx, &principal.room_id)
                .await
    {
        return Ok(AgentCreateExecution {
            reply: Err(CommandFailure::unresolved(error)),
            committed_events: effect.newly_committed_events,
            advance_ordered_floor: false,
        });
    }
    let reservation = match adapter.reserve_start(&effect.session).await {
        Ok(reservation) => reservation,
        Err(error) => {
            return fail_created_agent_start_before_effect(
                store,
                principal,
                request_id,
                payload,
                &effect,
                (&error.code, &error.message),
            )
            .await;
        }
    };
    let authorized = store
        .authorize_agent_create_start_effect(
            command.mutation_authority(),
            request_id,
            payload,
            &effect.operation_id,
            (
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                &reservation.runtime_lease_token,
            ),
        )
        .await;
    let authorized = match authorized {
        Ok(effect) => effect,
        Err(error) => {
            adapter
                .cancel_start_reservation(
                    &effect.session.public.room_id,
                    &effect.session.public.session_id,
                    &reservation,
                )
                .await;
            if let Some(reason) = ended_session_authority(&error) {
                return fail_created_agent_start_before_effect(
                    store, principal, request_id, payload, &effect, reason,
                )
                .await;
            }
            return Err(CommandFailure::unresolved(error));
        }
    };
    match adapter.start_reserved(&authorized.session, None).await {
        Ok(started) => {
            complete_created_agent_start(
                store,
                principal,
                request_id,
                payload,
                &authorized,
                started,
            )
            .await
        }
        Err(error) => {
            fail_created_agent_start(
                store,
                adapter,
                principal,
                request_id,
                payload,
                &authorized,
                error,
            )
            .await
        }
    }
}

async fn resolve_create_start_plan(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    command: &RoomCommand,
) -> Result<AgentCreateStartPlan, CommandFailure> {
    let principal = &command.principal;
    let request_id = command.request_id.as_str();
    let payload = &command.payload;
    let mut inspected = store
        .inspect_agent_create_start(command.mutation_authority(), request_id, payload)
        .await;
    if inspected.as_ref().is_err_and(|error| {
        matches!(
            error,
            PersistenceError::CommandUnresolved { code, .. } if matches!(code.as_bytes(), b"runtime_effect_unconfirmed")
        )
    }) {
        inspected = match recover_exact_lifecycle_command(
            store,
            provider_adapter,
            principal,
            request_id,
            "agent.create",
            payload,
        )
        .await
        {
            Ok(LiveRuntimeReconciliation::RetryOriginalEffect) => {
                store
                    .inspect_agent_create_start(command.mutation_authority(), request_id, payload)
                    .await
            }
            Ok(LiveRuntimeReconciliation::StillUnresolved) => inspected,
            Err(error) => Err(error),
        };
    }
    Ok(match inspected.map_err(CommandFailure::transactional)? {
        AgentCreateStartPlan::Select => {
            let selection = provider_catalog
                .validate_creation(
                    &principal.room_id,
                    &principal.principal_id,
                    request_id,
                    payload,
                )
                .await
                .map_err(|error| CommandFailure::rejected(selection_error(error)))?;
            if !selection.start_requested {
                return Err(CommandFailure::rejected(rejected(
                    "invalid_state",
                    "Provider selection lost the create/start intent.",
                )));
            }
            store
                .prepare_agent_create_start(
                    command.mutation_authority(),
                    request_id,
                    payload,
                    &selection.into(),
                )
                .await
                .map_err(CommandFailure::transactional)?
        }
        plan => plan,
    })
}

async fn fail_created_agent_start_before_effect(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
    effect: &AgentCreateStartEffect,
    reason: (&str, &str),
) -> Result<AgentCreateExecution, CommandFailure> {
    let commit = store
        .fail_agent_create_start_before_effect(
            principal, request_id, payload, effect, reason.0, reason.1,
        )
        .await
        .map_err(CommandFailure::unresolved)?;
    Ok(AgentCreateExecution {
        reply: Err(CommandFailure::rejected(
            PersistenceError::StoredCommandRejected {
                code: commit.code,
                message: commit.message,
            },
        )),
        committed_events: commit.events,
        advance_ordered_floor: false,
    })
}

async fn complete_created_agent_start(
    store: &SqliteStore,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
    effect: &AgentCreateStartEffect,
    started: ProviderRuntimeStarted,
) -> Result<AgentCreateExecution, CommandFailure> {
    let commit = store
        .complete_agent_create_start(
            principal,
            request_id,
            payload,
            &effect.operation_id,
            &persisted_start(started),
        )
        .await
        .map_err(CommandFailure::unresolved)?;
    Ok(AgentCreateExecution {
        reply: Ok(commit.outcome),
        committed_events: commit.newly_committed_events,
        advance_ordered_floor: true,
    })
}

async fn fail_created_agent_start(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
    effect: &AgentCreateStartEffect,
    error: ProviderAdapterError,
) -> Result<AgentCreateExecution, CommandFailure> {
    let (events, failure) = if error.effect_uncertain {
        let events = store
            .mark_agent_start_unconfirmed(
                principal,
                &effect.session.public.session_id,
                &effect.operation_id,
                &error.runtime_handle_id,
                &error.runtime_owner_id,
                &error.code,
                &error.message,
            )
            .await
            .map_err(CommandFailure::unresolved)?;
        (
            events,
            CommandFailure::unresolved(rejected(error.code, error.message)),
        )
    } else {
        let commit = store
            .fail_agent_create_start(
                principal,
                request_id,
                payload,
                effect,
                &error.code,
                &error.message,
            )
            .await
            .map_err(CommandFailure::unresolved)?;
        provider_adapter
            .release_checkpointed_start_absence(&effect.session)
            .await;
        (
            commit.events,
            CommandFailure::rejected(PersistenceError::StoredCommandRejected {
                code: commit.code,
                message: commit.message,
            }),
        )
    };
    Ok(AgentCreateExecution {
        reply: Err(failure),
        committed_events: events,
        advance_ordered_floor: false,
    })
}

fn success(outcome: CommandOutcome, advance_ordered_floor: bool) -> AgentCreateExecution {
    let committed_events = if outcome.deduplicated {
        Vec::new()
    } else {
        outcome.events.clone()
    };
    AgentCreateExecution {
        reply: Ok(outcome),
        committed_events,
        advance_ordered_floor,
    }
}

fn selection_error(error: agentsassemble_provider::ProviderSelectionError) -> PersistenceError {
    rejected(error.code, error.message)
}

fn rejected(
    code: impl Into<std::borrow::Cow<'static, str>>,
    message: impl Into<String>,
) -> PersistenceError {
    PersistenceError::CommandRejected {
        code: code.into(),
        message: message.into(),
    }
}
