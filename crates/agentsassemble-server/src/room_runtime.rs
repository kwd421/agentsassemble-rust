use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use agentsassemble_domain::{
    AuthenticatedPrincipal, RoomEvent, public_event_for_principal, public_value_for_principal,
};
use agentsassemble_persistence::{
    AgentTurnAssignment, CommandOutcome, PersistenceError, RoomCommandMutation, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderCatalogService, ProviderRoomToolCommand, ProviderRoomToolIngress,
};
use serde_json::Value;
use thiserror::Error;
use tokio::{
    sync::{Mutex, broadcast, mpsc, oneshot},
    task::{JoinHandle, JoinSet},
    time::MissedTickBehavior,
};
use tokio_util::sync::CancellationToken;

use crate::{
    agent_create_runtime::AgentCreateExecution,
    principal_write_budget::PrincipalWriteBudget,
    provider_turn::{
        ProviderTurnTaskResult, commit_provider_result, publish_turn_commit, spawn_provider_turn,
    },
    room_command_result::CommandFailure,
    room_shutdown::join_room_tasks,
};

const ROOM_QUEUE_CAPACITY: usize = 128;
const ROOM_TOOL_QUEUE_CAPACITY: usize = 64;
const EVENT_RECEIVER_CAPACITY: usize = 256;
const PUBLICATION_WAKE_CAPACITY: usize = 128;
const PUBLICATION_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const ROOM_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoomShutdownError {
    #[error("room mutation tasks exceeded the shutdown deadline")]
    TimedOut,
    #[error("room mutation task failed: {0}")]
    TaskFailed(String),
    #[error("provider runtime shutdown failed: {0}")]
    Provider(String),
    #[error("confirmed provider shutdown checkpoint failed: {0}")]
    Persistence(String),
}

pub(crate) struct RoomCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) request_id: String,
    pub(crate) action: String,
    pub(crate) payload: Value,
    transport_bytes: usize,
    reply: oneshot::Sender<Result<CommandOutcome, CommandFailure>>,
}

#[derive(Clone)]
struct RoomHandle {
    commands: mpsc::Sender<RoomCommand>,
    events: broadcast::Sender<RoomEvent>,
    publication_wake: mpsc::Sender<()>,
}

struct RoomTaskContext {
    room_id: String,
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    cancellation: CancellationToken,
    event_tx: broadcast::Sender<RoomEvent>,
    room_tool_ingress: ProviderRoomToolIngress,
}

struct RoomCommandOwners<'a> {
    store: &'a SqliteStore,
    provider_catalog: &'a ProviderCatalogService,
    provider_adapter: &'a ProviderAdapter,
    event_tx: &'a broadcast::Sender<RoomEvent>,
    turn_tasks: &'a mut JoinSet<ProviderTurnTaskResult>,
    room_tool_ingress: &'a ProviderRoomToolIngress,
    write_budget: &'a mut PrincipalWriteBudget,
}

#[derive(Clone)]
pub struct RoomRuntime {
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    rooms: Arc<Mutex<HashMap<String, RoomHandle>>>,
    cancellation: CancellationToken,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl RoomRuntime {
    #[must_use]
    pub fn new(store: SqliteStore, provider_catalog: ProviderCatalogService) -> Self {
        Self::with_provider_adapter(store, provider_catalog, ProviderAdapter::new())
    }

    #[must_use]
    pub fn with_provider_adapter(
        store: SqliteStore,
        provider_catalog: ProviderCatalogService,
        provider_adapter: ProviderAdapter,
    ) -> Self {
        Self {
            store,
            provider_catalog,
            provider_adapter,
            rooms: Arc::new(Mutex::new(HashMap::new())),
            cancellation: CancellationToken::new(),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Enqueues one durable command on its room's single mutation task and reports whether a
    /// failure definitively rejected the command or left its durable outcome unresolved.
    pub(crate) async fn execute(
        &self,
        principal: AuthenticatedPrincipal,
        request_id: String,
        action: String,
        payload: Value,
        transport_bytes: usize,
    ) -> Result<CommandOutcome, CommandFailure> {
        let handle = self.handle(&principal.room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .commands
            .try_send(RoomCommand {
                principal,
                request_id,
                action,
                payload,
                transport_bytes,
                reply,
            })
            .map_err(|error| {
                CommandFailure::unresolved(match error {
                    mpsc::error::TrySendError::Full(_) => PersistenceError::CommandRejected {
                        code: "room_busy",
                        message: "Room command queue is full.".to_owned(),
                    },
                    mpsc::error::TrySendError::Closed(_) => PersistenceError::CommandRejected {
                        code: "room_unavailable",
                        message: "Room mutation task stopped.".to_owned(),
                    },
                })
            })?;
        response.await.map_err(|_| {
            CommandFailure::unresolved(PersistenceError::CommandRejected {
                code: "room_unavailable",
                message: "Room mutation response was lost.".to_owned(),
            })
        })?
    }

    pub async fn subscribe(&self, room_id: &str) -> broadcast::Receiver<RoomEvent> {
        self.handle(room_id).await.events.subscribe()
    }

    pub async fn notify_committed_events(&self, events: &[RoomEvent]) {
        let mut notified_rooms = HashSet::new();
        for event in events {
            if !notified_rooms.insert(event.room_id.clone()) {
                continue;
            }
            let handle = self.handle(&event.room_id).await;
            let _ = handle.publication_wake.try_send(());
        }
    }

    /// Cancels all room mutation owners and bounds their cooperative shutdown.
    ///
    /// # Errors
    ///
    /// Returns a visible timeout or task failure after aborting and joining every task.
    pub async fn shutdown(&self) -> Result<(), RoomShutdownError> {
        self.cancellation.cancel();
        let tasks = {
            let mut tasks = self.tasks.lock().await;
            std::mem::take(&mut *tasks)
        };
        let room_result = join_room_tasks(tasks, ROOM_SHUTDOWN_TIMEOUT).await;
        let provider_outcome = self.provider_adapter.shutdown_with_observations().await;
        let checkpoint_result = crate::runtime_reconciliation::checkpoint_confirmed_shutdowns(
            &self.store,
            &provider_outcome.gone,
        )
        .await
        .map_err(|error| RoomShutdownError::Persistence(error.to_string()));
        if checkpoint_result.is_ok() {
            self.provider_adapter
                .release_shutdown_observations(&provider_outcome.gone)
                .await;
        }
        let provider_result = provider_outcome.failure.map_or(Ok(()), |error| {
            Err(RoomShutdownError::Provider(error.to_string()))
        });
        room_result.and(provider_result).and(checkpoint_result)
    }

    async fn handle(&self, room_id: &str) -> RoomHandle {
        let mut rooms = self.rooms.lock().await;
        if let Some(handle) = rooms.get(room_id) {
            return handle.clone();
        }
        let (command_tx, command_rx) = mpsc::channel::<RoomCommand>(ROOM_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_RECEIVER_CAPACITY);
        let (publication_tx, publication_rx) = mpsc::channel(PUBLICATION_WAKE_CAPACITY);
        let (room_tool_ingress, room_tool_rx) =
            ProviderRoomToolIngress::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let handle = RoomHandle {
            commands: command_tx,
            events: event_tx.clone(),
            publication_wake: publication_tx,
        };
        rooms.insert(room_id.to_owned(), handle.clone());
        let store = self.store.clone();
        let provider_catalog = self.provider_catalog.clone();
        let provider_adapter = self.provider_adapter.clone();
        let cancellation = self.cancellation.clone();
        let task = spawn_room_task(
            RoomTaskContext {
                room_id: room_id.to_owned(),
                store,
                provider_catalog,
                provider_adapter,
                cancellation,
                event_tx,
                room_tool_ingress,
            },
            command_rx,
            publication_rx,
            room_tool_rx,
        );
        self.tasks.lock().await.push(task);
        handle
    }
}

fn spawn_room_task(
    context: RoomTaskContext,
    mut command_rx: mpsc::Receiver<RoomCommand>,
    mut publication_rx: mpsc::Receiver<()>,
    mut room_tool_rx: mpsc::Receiver<ProviderRoomToolCommand>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let RoomTaskContext {
            room_id,
            store,
            provider_catalog,
            provider_adapter,
            cancellation,
            event_tx,
            room_tool_ingress,
        } = context;
        let mut turn_tasks = JoinSet::new();
        let mut publication_retry = tokio::time::interval(PUBLICATION_RETRY_INTERVAL);
        let mut write_budget = PrincipalWriteBudget::new();
        publication_retry.set_missed_tick_behavior(MissedTickBehavior::Delay);
        publish_durable_room_events(&store, &event_tx, &room_id).await;
        loop {
            let input = tokio::select! {
                () = cancellation.cancelled() => {
                    turn_tasks.abort_all();
                    while turn_tasks.join_next().await.is_some() {}
                    break;
                }
                command = command_rx.recv() => {
                    let Some(command) = command else { break; };
                    RoomInput::Command(command)
                }
                wake = publication_rx.recv() => {
                    let Some(()) = wake else { break; };
                    RoomInput::Publication
                }
                _ = publication_retry.tick() => RoomInput::Publication,
                result = turn_tasks.join_next(), if !turn_tasks.is_empty() => {
                    let Some(result) = result else { continue; };
                    RoomInput::Provider(Box::new(result))
                }
                tool = room_tool_rx.recv() => {
                    let Some(tool) = tool else { break; };
                    RoomInput::Tool(tool)
                }
            };
            match input {
                RoomInput::Command(command) => {
                    handle_room_command(
                        RoomCommandOwners {
                            store: &store,
                            provider_catalog: &provider_catalog,
                            provider_adapter: &provider_adapter,
                            event_tx: &event_tx,
                            turn_tasks: &mut turn_tasks,
                            room_tool_ingress: &room_tool_ingress,
                            write_budget: &mut write_budget,
                        },
                        command,
                    )
                    .await;
                }
                RoomInput::Provider(result) => {
                    handle_provider_result(
                        &store,
                        &provider_adapter,
                        &event_tx,
                        &mut turn_tasks,
                        *result,
                        &room_tool_ingress,
                    )
                    .await;
                }
                RoomInput::Tool(command) => {
                    crate::room_random_runtime::handle_provider_room_tool(
                        &store,
                        &event_tx,
                        &room_id,
                        command,
                        &mut write_budget,
                    )
                    .await;
                }
                RoomInput::Publication => {
                    publish_durable_room_events(&store, &event_tx, &room_id).await;
                }
            }
        }
    })
}

async fn handle_room_command(owners: RoomCommandOwners<'_>, mut command: RoomCommand) {
    let RoomCommandOwners {
        store,
        provider_catalog,
        provider_adapter,
        event_tx,
        turn_tasks,
        room_tool_ingress,
        write_budget,
    } = owners;
    let execution = match write_budget
        .admit_transport(&command.principal.principal_id, command.transport_bytes)
    {
        Ok(()) => match validate_command_envelope(&command) {
            Ok(()) => match store.resolve_principal(&command.principal).await {
                Ok(principal) => {
                    command.principal = principal;
                    match write_budget
                        .admit_command(
                            store,
                            &command.principal,
                            &command.request_id,
                            &command.action,
                            &command.payload,
                        )
                        .await
                    {
                        Ok(()) => {
                            execute_command(
                                store,
                                provider_catalog,
                                provider_adapter,
                                event_tx,
                                &command,
                            )
                            .await
                        }
                        Err(error) => CommandExecution::admission_failure(error),
                    }
                }
                Err(error) => CommandExecution::unresolved_failure(error),
            },
            Err(error) => CommandExecution::rejected_failure(error),
        },
        Err(error) => CommandExecution::unresolved_failure(error),
    };
    let CommandExecution {
        reply,
        committed_events,
        assignments,
    } = execution;
    if !committed_events.is_empty() {
        publish_durable_room_events(store, event_tx, &command.principal.room_id).await;
    }
    for assignment in assignments {
        spawn_provider_turn(
            turn_tasks,
            provider_adapter.clone(),
            assignment,
            room_tool_ingress.clone(),
        );
    }
    let reply = match reply {
        Ok(outcome) => {
            public_command_outcome(&command.principal, outcome).map_err(CommandFailure::unresolved)
        }
        Err(failure) => Err(failure),
    };
    let _ = command.reply.send(reply);
}

fn validate_command_envelope(command: &RoomCommand) -> Result<(), PersistenceError> {
    if command.request_id.is_empty()
        || command.request_id.chars().count() > 128
        || command.action.is_empty()
        || command.action.chars().count() > 64
    {
        return Err(PersistenceError::CommandRejected {
            code: "command_envelope_invalid",
            message: "request_id or action is invalid.".to_owned(),
        });
    }
    Ok(())
}

async fn handle_provider_result(
    store: &SqliteStore,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    turn_tasks: &mut JoinSet<ProviderTurnTaskResult>,
    result: Result<ProviderTurnTaskResult, tokio::task::JoinError>,
    room_tool_ingress: &ProviderRoomToolIngress,
) {
    let result = match result {
        Ok(result) => result,
        Err(join_error) => {
            tracing::error!(
                cancelled = join_error.is_cancelled(),
                panic = join_error.is_panic(),
                "provider turn task ended without a result; durable restart recovery is required"
            );
            return;
        }
    };
    let room_id = result.assignment.session.public.room_id.clone();
    let session_id = result.assignment.session.public.session_id.clone();
    match commit_provider_result(store, provider_adapter, result).await {
        Ok(commit) => {
            publish_turn_commit(
                store,
                event_tx,
                turn_tasks,
                provider_adapter.clone(),
                room_tool_ingress.clone(),
                commit,
            )
            .await;
        }
        Err(PersistenceError::CommandRejected {
            code: "stale_provider_turn",
            ..
        }) => tracing::debug!(
            room_id,
            session_id,
            "discarded provider result after durable turn authority changed"
        ),
        Err(_) => tracing::error!(
            room_id,
            session_id,
            "provider turn result could not be committed; durable restart recovery is required"
        ),
    }
}

async fn publish_durable_room_events(
    store: &SqliteStore,
    event_tx: &broadcast::Sender<RoomEvent>,
    room_id: &str,
) {
    if let Err(error) =
        crate::event_publication::drain_room_publications(store, event_tx, room_id).await
    {
        tracing::error!(
            error = ?error,
            room_id,
            "durable room-event publication failed; the room owner will retry"
        );
    }
}

fn public_command_outcome(
    principal: &AuthenticatedPrincipal,
    mut outcome: CommandOutcome,
) -> Result<CommandOutcome, PersistenceError> {
    outcome.result = public_value_for_principal(&outcome.result, principal)?;
    outcome.event = public_event_for_principal(&outcome.event, principal);
    outcome.events = outcome
        .events
        .iter()
        .map(|event| public_event_for_principal(event, principal))
        .collect();
    Ok(outcome)
}

enum RoomInput {
    Command(RoomCommand),
    Provider(Box<Result<ProviderTurnTaskResult, tokio::task::JoinError>>),
    Publication,
    Tool(ProviderRoomToolCommand),
}

async fn execute_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> CommandExecution {
    match command.action.as_str() {
        "agent.create" => {
            execute_agent_create_command(
                store,
                provider_catalog,
                provider_adapter,
                event_tx,
                command,
            )
            .await
        }
        "agent.configure" => execute_agent_configure(store, provider_catalog, command)
            .await
            .unwrap_or_else(CommandExecution::transactional_failure),
        "agent.start" | "agent.resume" => {
            crate::room_agent_lifecycle_runtime::execute_agent_start(
                store,
                provider_adapter,
                command,
            )
            .await
        }
        "agent.stop" => {
            crate::room_agent_lifecycle_runtime::execute_agent_stop(
                store,
                provider_adapter,
                command,
            )
            .await
        }
        "room.settings.update" => match store
            .execute_room_settings_update(&command.principal, &command.request_id, &command.payload)
            .await
        {
            Ok(outcome) if outcome.deduplicated => CommandExecution::success(outcome),
            Ok(outcome) => progressed_execution(store, &command.principal.room_id, outcome).await,
            Err(error) => CommandExecution::transactional_failure(error),
        },
        "room.random.roll" | "room.random.choose" => {
            match crate::room_random_runtime::execute_room_random(store, command).await {
                Ok(outcome) => CommandExecution::success(outcome),
                Err(error) => CommandExecution::transactional_failure(error),
            }
        }
        _ => match store
            .execute_message_with_turn(
                &command.principal,
                &command.request_id,
                &command.action,
                &command.payload,
            )
            .await
        {
            Ok(mutation) => CommandExecution::mutation(mutation),
            Err(error) => CommandExecution::transactional_failure(error),
        },
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
            &command.action,
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

pub(crate) struct CommandExecution {
    reply: Result<CommandOutcome, CommandFailure>,
    committed_events: Vec<RoomEvent>,
    assignments: Vec<AgentTurnAssignment>,
}

impl CommandExecution {
    fn success(outcome: CommandOutcome) -> Self {
        let committed_events = if outcome.deduplicated {
            Vec::new()
        } else {
            outcome.events.clone()
        };
        Self {
            reply: Ok(outcome),
            committed_events,
            assignments: Vec::new(),
        }
    }

    fn mutation(mutation: RoomCommandMutation) -> Self {
        let committed_events = if mutation.outcome.deduplicated {
            Vec::new()
        } else {
            mutation.outcome.events.clone()
        };
        Self {
            reply: Ok(mutation.outcome),
            committed_events,
            assignments: mutation.assignments,
        }
    }

    fn failure(failure: CommandFailure) -> Self {
        Self {
            reply: Err(failure),
            committed_events: Vec::new(),
            assignments: Vec::new(),
        }
    }

    pub(crate) fn transactional_failure(error: PersistenceError) -> Self {
        Self::failure(CommandFailure::transactional(error))
    }

    fn rejected_failure(error: PersistenceError) -> Self {
        Self {
            reply: Err(CommandFailure::rejected(error)),
            committed_events: Vec::new(),
            assignments: Vec::new(),
        }
    }

    pub(crate) fn unresolved_failure(error: PersistenceError) -> Self {
        Self {
            reply: Err(CommandFailure::unresolved(error)),
            committed_events: Vec::new(),
            assignments: Vec::new(),
        }
    }

    pub(crate) fn unresolved_failure_with_events(
        error: PersistenceError,
        committed_events: Vec<RoomEvent>,
    ) -> Self {
        Self {
            reply: Err(CommandFailure::unresolved(error)),
            committed_events,
            assignments: Vec::new(),
        }
    }

    fn admission_failure(error: PersistenceError) -> Self {
        Self {
            reply: Err(CommandFailure::after_admission(error)),
            committed_events: Vec::new(),
            assignments: Vec::new(),
        }
    }

    pub(crate) fn committed_failure(
        error: PersistenceError,
        committed_events: Vec<RoomEvent>,
    ) -> Self {
        Self {
            reply: Err(CommandFailure::rejected(error)),
            committed_events,
            assignments: Vec::new(),
        }
    }
}

pub(crate) async fn progressed_execution(
    store: &SqliteStore,
    room_id: &str,
    outcome: CommandOutcome,
) -> CommandExecution {
    progress_execution(store, room_id, CommandExecution::success(outcome)).await
}

async fn progress_execution(
    store: &SqliteStore,
    room_id: &str,
    mut execution: CommandExecution,
) -> CommandExecution {
    match store.assign_pending_turn(room_id).await {
        Ok(Some(commit)) => {
            execution.committed_events.extend(commit.events);
            execution.assignments.extend(commit.next_assignments);
        }
        Ok(None) => {}
        Err(error) => {
            let code = match &error {
                PersistenceError::CommandRejected { code, .. } => *code,
                _ => "persistence_error",
            };
            tracing::error!(
                code,
                room_id,
                "committed lifecycle command could not advance the ordered floor"
            );
            match store.record_floor_progression_failure(room_id, code).await {
                Ok(events) => execution.committed_events.extend(events),
                Err(recording_error) => tracing::error!(
                    error = ?recording_error,
                    room_id,
                    "room floor progression failure could not be recorded durably"
                ),
            }
        }
    }
    execution
}
