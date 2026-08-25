use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{
    AgentCreateStartEffect, AgentCreateStartPlan, AgentRuntimeStarted, CommandOutcome,
    LiveRuntimeReconciliation, PersistenceError, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAdapterError, ProviderCatalogService, ProviderRuntimeStarted,
    creation_start_requested,
};
use serde_json::Value;
use tokio::sync::broadcast;

use crate::{
    room_command_result::CommandFailure, runtime_reconciliation::recover_exact_lifecycle_command,
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
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
) -> Result<AgentCreateExecution, CommandFailure> {
    let start_requested = creation_start_requested(payload)
        .map_err(|error| CommandFailure::rejected(selection_error(error)))?;
    if start_requested {
        return execute_agent_create_start(
            store,
            provider_catalog,
            provider_adapter,
            event_tx,
            principal,
            request_id,
            payload,
        )
        .await;
    }
    if let Some(outcome) = store
        .replay_command(principal, request_id, "agent.create", payload)
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
        .execute_agent_create(principal, request_id, payload, &selection.into())
        .await
        .map_err(CommandFailure::transactional)?;
    Ok(success(outcome, false))
}

async fn execute_agent_create_start(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
) -> Result<AgentCreateExecution, CommandFailure> {
    let plan = resolve_create_start_plan(
        store,
        provider_catalog,
        provider_adapter,
        principal,
        request_id,
        payload,
    )
    .await?;
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
    if !effect.newly_committed_events.is_empty() {
        crate::event_publication::drain_room_publications(store, event_tx, &principal.room_id)
            .await
            .map_err(CommandFailure::unresolved)?;
    }
    let reservation = match provider_adapter.reserve_start(&effect.session).await {
        Ok(reservation) => reservation,
        Err(error) => {
            return fail_created_agent_start_before_effect(
                store, principal, request_id, payload, &effect, error,
            )
            .await;
        }
    };
    let authorized = store
        .authorize_agent_create_start_effect(
            principal,
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
            provider_adapter
                .cancel_start_reservation(
                    &effect.session.public.room_id,
                    &effect.session.public.session_id,
                    &reservation,
                )
                .await;
            return Err(CommandFailure::unresolved(error));
        }
    };
    match provider_adapter.start_reserved(&authorized.session).await {
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
            fail_created_agent_start(store, principal, request_id, payload, &authorized, error)
                .await
        }
    }
}

async fn resolve_create_start_plan(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    principal: &AuthenticatedPrincipal,
    request_id: &str,
    payload: &Value,
) -> Result<AgentCreateStartPlan, CommandFailure> {
    let mut inspected = store
        .inspect_agent_create_start(principal, request_id, payload)
        .await;
    if inspected.as_ref().is_err_and(|error| {
        matches!(
            error,
            PersistenceError::CommandUnresolved {
                code: "runtime_effect_unconfirmed",
                ..
            }
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
                    .inspect_agent_create_start(principal, request_id, payload)
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
                .prepare_agent_create_start(principal, request_id, payload, &selection.into())
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
    error: ProviderAdapterError,
) -> Result<AgentCreateExecution, CommandFailure> {
    let commit = store
        .fail_agent_create_start_before_effect(
            principal,
            request_id,
            payload,
            effect,
            error.code,
            error.message,
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
                error.code,
                error.message,
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
                error.code,
                error.message,
            )
            .await
            .map_err(CommandFailure::unresolved)?;
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

fn selection_error(error: agentsassemble_provider::ProviderSelectionError) -> PersistenceError {
    rejected(error.code, error.message)
}

fn rejected(code: &'static str, message: impl Into<String>) -> PersistenceError {
    PersistenceError::CommandRejected {
        code,
        message: message.into(),
    }
}
