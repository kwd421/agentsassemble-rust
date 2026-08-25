use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{
    AgentTurnAssignment, CommandOutcome, PersistenceError, SqliteStore,
};
use agentsassemble_protocol::RoomAction;
use agentsassemble_provider::{
    ProviderAdapter, ProviderCatalogService, ProviderRoomToolCommand, ProviderRoomToolIngress,
};
use serde_json::Value;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, broadcast, mpsc, oneshot},
    task::{JoinHandle, JoinSet},
    time::MissedTickBehavior,
};
use tokio_util::sync::CancellationToken;

use crate::{
    agent_create_runtime::AgentCreateExecution,
    lifecycle_command_tracker::LifecycleCommandTracker,
    principal_mutation_admission::{MutationDebit, PrincipalMutationAdmission},
    provider_turn::{
        ProviderTurnTaskResult, commit_provider_result, publish_turn_commit, spawn_provider_turn,
    },
    provider_write_budget::ProviderWriteBudget,
    room_command_admission::{AdmittedHumanCommand, admit_human_command},
    room_command_result::{CommandFailure, public_command_outcome},
    room_shutdown::{RoomShutdownError, join_room_tasks},
};

use crate::room_command_execution::persistence_error_code;
pub(crate) use crate::room_command_execution::{
    CommandExecution, progress_execution, progressed_execution,
};

const ROOM_QUEUE_CAPACITY: usize = 128;
const ROOM_TOOL_QUEUE_CAPACITY: usize = 64;
const EVENT_RECEIVER_CAPACITY: usize = 256;
const PUBLICATION_WAKE_CAPACITY: usize = 128;
const PUBLICATION_RETRY_INTERVAL: Duration = Duration::from_millis(250);
const ROOM_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct RoomCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) request_id: String,
    pub(crate) action: RoomAction,
    pub(crate) payload: Value,
    mutation_debit: Option<MutationDebit>,
    _inflight_permit: OwnedSemaphorePermit,
    reply: oneshot::Sender<Result<CommandOutcome, CommandFailure>>,
}

#[derive(Clone)]
struct RoomHandle {
    commands: mpsc::Sender<RoomCommand>,
    events: broadcast::Sender<RoomEvent>,
    publication_wake: mpsc::Sender<()>,
    provider_recovery: mpsc::Sender<AgentTurnAssignment>,
}

struct RoomTaskContext {
    room_id: String,
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    cancellation: CancellationToken,
    event_tx: broadcast::Sender<RoomEvent>,
    room_tool_ingress: ProviderRoomToolIngress,
    lifecycle_commands: LifecycleCommandTracker,
}

struct RoomCommandOwners<'a> {
    store: &'a SqliteStore,
    provider_catalog: &'a ProviderCatalogService,
    provider_adapter: &'a ProviderAdapter,
    event_tx: &'a broadcast::Sender<RoomEvent>,
    turn_tasks: &'a mut JoinSet<ProviderTurnTaskResult>,
    room_tool_ingress: &'a ProviderRoomToolIngress,
    lifecycle_commands: &'a LifecycleCommandTracker,
}

#[derive(Clone)]
pub struct RoomRuntime {
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    rooms: Arc<Mutex<HashMap<String, RoomHandle>>>,
    cancellation: CancellationToken,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
    lifecycle_commands: LifecycleCommandTracker,
    principal_mutations: PrincipalMutationAdmission,
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
            lifecycle_commands: LifecycleCommandTracker::default(),
            principal_mutations: PrincipalMutationAdmission::new(),
        }
    }

    /// Enqueues one durable command on its room owner and classifies its outcome.
    pub(crate) async fn execute(
        &self,
        principal: AuthenticatedPrincipal,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let AdmittedHumanCommand {
            principal,
            mutation_debit,
            inflight_permit,
        } = admit_human_command(
            &self.store,
            &self.principal_mutations,
            &principal,
            &request_id,
            action,
            &payload,
        )
        .await?;
        let handle = self.handle(&principal.room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .commands
            .try_send(RoomCommand {
                principal,
                request_id,
                action,
                payload,
                mutation_debit,
                _inflight_permit: inflight_permit,
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

    pub(crate) async fn notify_room_publication(&self, room_id: &str) {
        let handle = self.handle(room_id).await;
        let _ = handle.publication_wake.try_send(());
    }

    pub(crate) async fn resume_assigned_provider_turn(
        &self,
        assignment: AgentTurnAssignment,
    ) -> Result<(), PersistenceError> {
        let handle = self.handle(&assignment.session.public.room_id).await;
        handle
            .provider_recovery
            .try_send(assignment)
            .map_err(|error| PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_unavailable",
                message: match error {
                    mpsc::error::TrySendError::Full(_) => {
                        "The provider turn recovery queue is full."
                    }
                    mpsc::error::TrySendError::Closed(_) => {
                        "The provider turn recovery owner stopped."
                    }
                }
                .to_owned(),
            })
    }

    pub(crate) fn try_claim_lifecycle_command(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        action: &str,
    ) -> Option<crate::lifecycle_command_tracker::LifecycleCommandGuard> {
        self.lifecycle_commands
            .try_claim(room_id, principal_id, request_id, action)
    }

    #[cfg(test)]
    pub(crate) fn claim_lifecycle_command(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        action: &str,
    ) -> crate::lifecycle_command_tracker::LifecycleCommandGuard {
        self.lifecycle_commands
            .try_claim(room_id, principal_id, request_id, action)
            .unwrap_or_else(|| panic!("test lifecycle command already has an owner"))
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
        let (provider_recovery_tx, provider_recovery_rx) = mpsc::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let (room_tool_ingress, room_tool_rx) =
            ProviderRoomToolIngress::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let handle = RoomHandle {
            commands: command_tx,
            events: event_tx.clone(),
            publication_wake: publication_tx,
            provider_recovery: provider_recovery_tx,
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
                lifecycle_commands: self.lifecycle_commands.clone(),
            },
            command_rx,
            publication_rx,
            room_tool_rx,
            provider_recovery_rx,
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
    mut provider_recovery_rx: mpsc::Receiver<AgentTurnAssignment>,
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
            lifecycle_commands,
        } = context;
        let mut turn_tasks = JoinSet::new();
        let mut publication_retry = tokio::time::interval(PUBLICATION_RETRY_INTERVAL);
        let mut provider_write_budget = ProviderWriteBudget::new();
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
                recovery = provider_recovery_rx.recv() => {
                    let Some(assignment) = recovery else { break; };
                    RoomInput::ProviderRecovery(Box::new(assignment))
                }
            };
            match input {
                RoomInput::Command(command) => {
                    Box::pin(handle_room_command(
                        RoomCommandOwners {
                            store: &store,
                            provider_catalog: &provider_catalog,
                            provider_adapter: &provider_adapter,
                            event_tx: &event_tx,
                            turn_tasks: &mut turn_tasks,
                            room_tool_ingress: &room_tool_ingress,
                            lifecycle_commands: &lifecycle_commands,
                        },
                        command,
                    ))
                    .await;
                }
                RoomInput::Provider(result) => {
                    Box::pin(handle_provider_result(
                        &store,
                        &provider_adapter,
                        &event_tx,
                        &mut turn_tasks,
                        *result,
                        &room_tool_ingress,
                    ))
                    .await;
                }
                RoomInput::Tool(command) => {
                    crate::room_random_runtime::handle_provider_room_tool(
                        &store,
                        &event_tx,
                        &room_id,
                        command,
                        &mut provider_write_budget,
                    )
                    .await;
                }
                RoomInput::ProviderRecovery(assignment) => {
                    spawn_provider_turn(
                        &mut turn_tasks,
                        store.clone(),
                        provider_adapter.clone(),
                        *assignment,
                        room_tool_ingress.clone(),
                    );
                }
                RoomInput::Publication => {
                    publish_durable_room_events(&store, &event_tx, &room_id).await;
                }
            }
        }
    })
}

async fn handle_room_command(owners: RoomCommandOwners<'_>, command: RoomCommand) {
    let RoomCommandOwners {
        store,
        provider_catalog,
        provider_adapter,
        event_tx,
        turn_tasks,
        room_tool_ingress,
        lifecycle_commands,
    } = owners;
    let lifecycle_guard = lifecycle_commands.try_claim(
        &command.principal.room_id,
        &command.principal.principal_id,
        &command.request_id,
        command.action.as_str(),
    );
    let execution = match lifecycle_guard {
        None => CommandExecution::unresolved_failure(PersistenceError::CommandUnresolved {
            code: "runtime_recovery_in_progress",
            message: "The exact lifecycle request is currently owned by server recovery. Retry the same request.".to_owned(),
        }),
        Some(_lifecycle_guard) => {
            Box::pin(execute_command(
                store,
                provider_catalog,
                provider_adapter,
                event_tx,
                &command,
            ))
            .await
        }
    };
    if execution.is_definitive()
        && let Some(debit) = &command.mutation_debit
    {
        debit.resolve();
    }
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
            store.clone(),
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
    if result.task_panicked {
        match store
            .record_provider_turn_task_death(
                &room_id,
                &session_id,
                result.assignment.turn_generation,
                &result.assignment.execution_id,
            )
            .await
        {
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
            Err(error) => tracing::error!(
                code = persistence_error_code(&error),
                room_id,
                session_id,
                "provider turn task death could not be checkpointed"
            ),
        }
        return;
    }
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

enum RoomInput {
    Command(RoomCommand),
    Provider(Box<Result<ProviderTurnTaskResult, tokio::task::JoinError>>),
    Publication,
    Tool(ProviderRoomToolCommand),
    ProviderRecovery(Box<AgentTurnAssignment>),
}

async fn execute_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    provider_adapter: &ProviderAdapter,
    event_tx: &broadcast::Sender<RoomEvent>,
    command: &RoomCommand,
) -> CommandExecution {
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
        RoomAction::MessageSend => match store
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
        },
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
                    match crate::participant_mute_runtime::apply_exact_interrupt(
                        store,
                        provider_adapter,
                        &effect,
                    )
                    .await
                    {
                        Ok(commit) => execution.extend_turn_commit(commit),
                        Err(error) => tracing::error!(
                            code = persistence_error_code(&error),
                            room_id = command.principal.room_id,
                            session_id = effect.session_id,
                            "participant mute committed; exact provider interrupt remains quarantined"
                        ),
                    }
                }
                execution
            }
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
