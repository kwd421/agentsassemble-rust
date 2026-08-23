use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{
    AgentCreateStartPlan, AgentRuntimeStarted, CommandOutcome, PersistenceError, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderCatalogService, ProviderRuntimeStarted, creation_start_requested,
};
use serde_json::Value;
use tokio::sync::broadcast;

pub(crate) struct AgentCreateExecution {
    pub reply: Result<CommandOutcome, PersistenceError>,
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
) -> Result<AgentCreateExecution, PersistenceError> {
    let start_requested = creation_start_requested(payload).map_err(selection_error)?;
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
        .await?
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
        .map_err(selection_error)?;
    let outcome = store
        .execute_agent_create(principal, request_id, payload, &selection.into())
        .await?;
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
) -> Result<AgentCreateExecution, PersistenceError> {
    let plan = match store
        .inspect_agent_create_start(principal, request_id, payload)
        .await?
    {
        AgentCreateStartPlan::Select => {
            let selection = provider_catalog
                .validate_creation(
                    &principal.room_id,
                    &principal.principal_id,
                    request_id,
                    payload,
                )
                .await
                .map_err(selection_error)?;
            if !selection.start_requested {
                return Err(rejected(
                    "invalid_state",
                    "Provider selection lost the create/start intent.",
                ));
            }
            store
                .prepare_agent_create_start(principal, request_id, payload, &selection.into())
                .await?
        }
        plan => plan,
    };
    let effect = match plan {
        AgentCreateStartPlan::Outcome(outcome) => return Ok(success(*outcome, false)),
        AgentCreateStartPlan::Start(effect) => effect,
        AgentCreateStartPlan::Select => {
            return Err(rejected(
                "invalid_state",
                "Create/start selection did not produce a durable intent.",
            ));
        }
    };
    publish_events(event_tx, &effect.newly_committed_events);
    match provider_adapter.start(&effect.session).await {
        Ok(started) => {
            let commit = store
                .complete_agent_create_start(
                    principal,
                    request_id,
                    payload,
                    &effect.operation_id,
                    &persisted_start(started),
                )
                .await?;
            Ok(AgentCreateExecution {
                reply: Ok(commit.outcome),
                committed_events: commit.newly_committed_events,
                advance_ordered_floor: true,
            })
        }
        Err(error) => {
            let events = if error.effect_uncertain {
                store
                    .mark_agent_start_unconfirmed(
                        principal,
                        &effect.session.public.session_id,
                        &effect.operation_id,
                        &error.runtime_handle_id,
                        &error.runtime_owner_id,
                        error.code,
                        error.message,
                    )
                    .await?
            } else {
                store
                    .fail_agent_create_start(
                        principal,
                        request_id,
                        payload,
                        &effect,
                        error.code,
                        error.message,
                    )
                    .await?
            };
            Ok(AgentCreateExecution {
                reply: Err(rejected(error.code, error.message)),
                committed_events: events,
                advance_ordered_floor: false,
            })
        }
    }
}

fn publish_events(event_tx: &broadcast::Sender<RoomEvent>, events: &[RoomEvent]) {
    for event in events {
        let _ = event_tx.send(event.clone());
    }
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
