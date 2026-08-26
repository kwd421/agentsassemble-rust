use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{
    AgentTurnAssignment, CommandOutcome, HumanAdmissionDecision, HumanAdmissionRejection,
    HumanSessionAuthorization, PersistenceError, PreparedHumanAdmission, SqliteStore,
};
use agentsassemble_protocol::RoomAction;
use agentsassemble_provider::{
    ProviderAdapter, ProviderCatalogService, ProviderRoomToolCommand, ProviderRoomToolIngress,
};
use serde_json::Value;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, broadcast, mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

use crate::{
    event_publication::{publish_durable_room_events, start_publication_owner},
    human_admission_runtime::{HumanAdmissionCommand, handle_human_admission},
    lifecycle_command_tracker::LifecycleCommandTracker,
    principal_mutation_admission::{MutationDebit, PrincipalMutationAdmission},
    provider_recovery_tracker::ProviderRecoveryTracker,
    provider_turn::{ProviderTurnTaskResult, handle_provider_result, spawn_provider_turn},
    provider_write_budget::ProviderWriteBudget,
    room_command_admission::{
        AdmittedHumanCommand, admit_human_command, admit_human_session_command,
    },
    room_command_result::{CommandFailure, public_command_outcome},
    room_recovery_runtime::{RecoveredAssignment, RecoveredAssignments, publish_then_resume},
    room_shutdown::{RoomShutdownError, join_room_tasks},
};

use crate::room_command_execution::CommandExecution;

const ROOM_QUEUE_CAPACITY: usize = 128;
const ROOM_TOOL_QUEUE_CAPACITY: usize = 64;
const EVENT_RECEIVER_CAPACITY: usize = 256;
const PUBLICATION_WAKE_CAPACITY: usize = 128;
const ROOM_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) struct RoomCommand {
    pub(crate) principal: AuthenticatedPrincipal,
    pub(crate) human_session: Option<Box<HumanSessionAuthorization>>,
    pub(crate) request_id: String,
    pub(crate) action: RoomAction,
    pub(crate) payload: Value,
    mutation_debit: Option<MutationDebit>,
    _inflight_permit: OwnedSemaphorePermit,
    reply: oneshot::Sender<Result<CommandOutcome, CommandFailure>>,
}

#[derive(Clone)]
struct RoomHandle {
    mutations: mpsc::Sender<RoomMutation>,
    events: broadcast::Sender<RoomEvent>,
    human_session_revocations: broadcast::Sender<[u8; 32]>,
    publication_wake: mpsc::Sender<()>,
    provider_recovery: mpsc::Sender<RecoveredAssignments>,
}

struct RoomTaskContext {
    room_id: String,
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    cancellation: CancellationToken,
    event_tx: broadcast::Sender<RoomEvent>,
    human_session_revocation_tx: broadcast::Sender<[u8; 32]>,
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
    provider_recoveries: ProviderRecoveryTracker,
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
            provider_recoveries: ProviderRecoveryTracker::default(),
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
        let admitted = admit_human_command(
            &self.store,
            &self.principal_mutations,
            &principal,
            &request_id,
            action,
            &payload,
        )
        .await?;
        self.enqueue_command(admitted, None, request_id, action, payload)
            .await
    }

    pub(crate) async fn execute_human_session(
        &self,
        authorization: &HumanSessionAuthorization,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let (admitted, current) = admit_human_session_command(
            &self.store,
            &self.principal_mutations,
            authorization,
            &request_id,
            action,
            &payload,
        )
        .await?;
        self.enqueue_command(
            admitted,
            Some(Box::new(current)),
            request_id,
            action,
            payload,
        )
        .await
    }

    async fn enqueue_command(
        &self,
        admitted: AdmittedHumanCommand,
        human_session: Option<Box<HumanSessionAuthorization>>,
        request_id: String,
        action: RoomAction,
        payload: Value,
    ) -> Result<CommandOutcome, CommandFailure> {
        let AdmittedHumanCommand {
            principal,
            mutation_debit,
            inflight_permit,
        } = admitted;
        let handle = self.handle(&principal.room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::Command(RoomCommand {
                principal,
                human_session,
                request_id,
                action,
                payload,
                mutation_debit,
                _inflight_permit: inflight_permit,
                reply,
            }))
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

    /// Enqueues one prepared human admission on the room's bounded mutation owner.
    ///
    /// # Errors
    ///
    /// Fails when routing, queue custody, or the admission transaction fails.
    pub async fn admit_human(
        &self,
        request: PreparedHumanAdmission,
    ) -> Result<HumanAdmissionDecision, PersistenceError> {
        let Some(room_id) = self.store.human_admission_room_id(&request).await? else {
            return Ok(HumanAdmissionDecision::Rejected(
                HumanAdmissionRejection::InviteNotFound,
            ));
        };
        let handle = self.handle(&room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .mutations
            .try_send(RoomMutation::HumanAdmission(HumanAdmissionCommand {
                request,
                reply,
            }))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PersistenceError::CommandRejected {
                    code: "room_busy",
                    message: "Room mutation queue is full.".to_owned(),
                },
                mpsc::error::TrySendError::Closed(_) => PersistenceError::CommandRejected {
                    code: "room_unavailable",
                    message: "Room mutation task stopped.".to_owned(),
                },
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandRejected {
                code: "room_unavailable",
                message: "Room admission response was lost.".to_owned(),
            })?
    }

    pub async fn subscribe(&self, room_id: &str) -> broadcast::Receiver<RoomEvent> {
        self.handle(room_id).await.events.subscribe()
    }

    /// Subscribes to post-commit replacement of human session fingerprints.
    pub async fn session_revocations(&self, room_id: &str) -> broadcast::Receiver<[u8; 32]> {
        self.handle(room_id)
            .await
            .human_session_revocations
            .subscribe()
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

    pub(crate) async fn publish_then_resume_assigned_turns(
        &self,
        room_id: &str,
        assignments: Vec<AgentTurnAssignment>,
    ) -> Result<(), PersistenceError> {
        if assignments
            .iter()
            .any(|assignment| assignment.session.public.room_id != room_id)
        {
            return Err(PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_authority_invalid",
                message: "Recovered provider assignments do not share one room authority."
                    .to_owned(),
            });
        }
        let assignments = assignments
            .into_iter()
            .filter_map(|assignment| {
                self.provider_recoveries
                    .try_claim(&assignment)
                    .map(|guard| RecoveredAssignment { assignment, guard })
            })
            .collect::<Vec<_>>();
        if assignments.is_empty() {
            return Ok(());
        }
        let handle = self.handle(room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .provider_recovery
            .send(RecoveredAssignments { assignments, reply })
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_unavailable",
                message: "The provider turn recovery owner stopped.".to_owned(),
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_unavailable",
                message: "The provider turn recovery owner lost its completion response."
                    .to_owned(),
            })?
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
        let mut checkpoint_result = Ok(());
        for stopped in &provider_outcome.gone {
            match Box::pin(
                crate::runtime_reconciliation::checkpoint_confirmed_shutdown(&self.store, stopped),
            )
            .await
            {
                Ok(()) => {
                    self.provider_adapter
                        .release_shutdown_observations(std::slice::from_ref(stopped))
                        .await;
                }
                Err(error) if checkpoint_result.is_ok() => {
                    checkpoint_result = Err(RoomShutdownError::Persistence(error.to_string()));
                }
                Err(_) => {}
            }
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
        let (mutation_tx, mutation_rx) = mpsc::channel::<RoomMutation>(ROOM_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_RECEIVER_CAPACITY);
        let (human_session_revocation_tx, _) = broadcast::channel(EVENT_RECEIVER_CAPACITY);
        let (publication_tx, publication_rx) = mpsc::channel(PUBLICATION_WAKE_CAPACITY);
        let (provider_recovery_tx, provider_recovery_rx) = mpsc::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let (room_tool_ingress, room_tool_rx) =
            ProviderRoomToolIngress::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let handle = RoomHandle {
            mutations: mutation_tx,
            events: event_tx.clone(),
            human_session_revocations: human_session_revocation_tx.clone(),
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
                human_session_revocation_tx,
                room_tool_ingress,
                lifecycle_commands: self.lifecycle_commands.clone(),
            },
            mutation_rx,
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
    mut mutation_rx: mpsc::Receiver<RoomMutation>,
    mut publication_rx: mpsc::Receiver<()>,
    mut room_tool_rx: mpsc::Receiver<ProviderRoomToolCommand>,
    mut provider_recovery_rx: mpsc::Receiver<RecoveredAssignments>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let RoomTaskContext {
            room_id,
            store,
            provider_catalog,
            provider_adapter,
            cancellation,
            event_tx,
            human_session_revocation_tx,
            room_tool_ingress,
            lifecycle_commands,
        } = context;
        let mut turn_tasks = JoinSet::new();
        let mut publication_retry = start_publication_owner(&store, &event_tx, &room_id).await;
        let mut provider_write_budget = ProviderWriteBudget::new();
        loop {
            let input = tokio::select! {
                () = cancellation.cancelled() => {
                    abort_provider_turns(&mut turn_tasks).await;
                    break;
                }
                mutation = mutation_rx.recv() => {
                    let Some(mutation) = mutation else { break; };
                    RoomInput::Mutation(mutation)
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
                RoomInput::Mutation(mutation) => {
                    handle_room_mutation(
                        RoomCommandOwners {
                            store: &store,
                            provider_catalog: &provider_catalog,
                            provider_adapter: &provider_adapter,
                            event_tx: &event_tx,
                            turn_tasks: &mut turn_tasks,
                            room_tool_ingress: &room_tool_ingress,
                            lifecycle_commands: &lifecycle_commands,
                        },
                        &room_id,
                        &human_session_revocation_tx,
                        mutation,
                    )
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
                    publish_then_resume(
                        &store,
                        &event_tx,
                        &room_id,
                        &mut turn_tasks,
                        &provider_adapter,
                        &room_tool_ingress,
                        *assignment,
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

async fn abort_provider_turns(turn_tasks: &mut JoinSet<ProviderTurnTaskResult>) {
    turn_tasks.abort_all();
    while turn_tasks.join_next().await.is_some() {}
}

async fn handle_room_mutation(
    owners: RoomCommandOwners<'_>,
    room_id: &str,
    session_revocations: &broadcast::Sender<[u8; 32]>,
    mutation: RoomMutation,
) {
    match mutation {
        RoomMutation::Command(command) => {
            Box::pin(handle_room_command(owners, session_revocations, command)).await;
        }
        RoomMutation::HumanAdmission(command) => {
            handle_human_admission(
                owners.store,
                room_id,
                owners.event_tx,
                session_revocations,
                command,
            )
            .await;
        }
    }
}

async fn handle_room_command(
    owners: RoomCommandOwners<'_>,
    session_revocations: &broadcast::Sender<[u8; 32]>,
    command: RoomCommand,
) {
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
            Box::pin(crate::room_command_dispatch::execute_command(
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
        revoked_human_sessions,
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
    for fingerprint in revoked_human_sessions {
        let _ = session_revocations.send(fingerprint);
    }
    let reply = match reply {
        Ok(outcome) => {
            public_command_outcome(&command.principal, outcome).map_err(CommandFailure::unresolved)
        }
        Err(failure) => Err(failure),
    };
    let _ = command.reply.send(reply);
}

enum RoomInput {
    Mutation(RoomMutation),
    Provider(Box<Result<ProviderTurnTaskResult, tokio::task::JoinError>>),
    Publication,
    Tool(ProviderRoomToolCommand),
    ProviderRecovery(Box<RecoveredAssignments>),
}

enum RoomMutation {
    Command(RoomCommand),
    HumanAdmission(HumanAdmissionCommand),
}
