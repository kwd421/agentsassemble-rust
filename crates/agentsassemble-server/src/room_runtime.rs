use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};

use agentsassemble_domain::RoomEvent;
use agentsassemble_persistence::{
    AgentTurnAssignment, HumanAdmissionDecision, HumanAdmissionRejection, PersistenceError,
    PreparedHumanAdmission, SqliteStore,
};
use agentsassemble_provider::{
    ProviderAdapter, ProviderAttachmentReadCommand, ProviderAttachmentReadIngress,
    ProviderCatalogService, ProviderRoomToolCommand, ProviderRoomToolIngress,
};
use tokio::{
    sync::{Mutex, broadcast, mpsc, oneshot},
    task::{JoinHandle, JoinSet},
};
use tokio_util::sync::CancellationToken;

use crate::{
    event_publication::{PublicationAttempt, PublicationRetry, publish_durable_room_events},
    human_admission_runtime::{HumanAdmissionCommand, handle_human_admission},
    lifecycle_command_tracker::LifecycleCommandTracker,
    principal_mutation_admission::PrincipalMutationAdmission,
    provider_recovery_tracker::ProviderRecoveryTracker,
    provider_turn::{
        ProviderTurnIngress, ProviderTurnTaskResult, handle_provider_result, spawn_provider_turn,
    },
    provider_write_budget::ProviderWriteBudget,
    room_command_result::{CommandFailure, public_command_outcome},
    room_recovery_runtime::{RecoveredAssignment, RecoveredAssignments, RecoveryRuntime},
    room_shutdown::{RoomShutdownError, join_room_tasks},
};

use crate::room_command_execution::CommandExecution;

#[path = "room_command_queue.rs"]
mod command_queue;
pub(crate) use command_queue::{RoomCommand, RoomCommandSession};

#[path = "attendee_execution.rs"]
mod attendee_execution;
pub use attendee_execution::{AttendeeOperation, AttendeeOperationResult};

#[path = "attendee_runtime.rs"]
mod attendee;
#[path = "connector_runtime.rs"]
pub(crate) mod connector;

#[path = "side_chat_runtime.rs"]
mod side_chat;

#[path = "room_runtime_failure.rs"]
mod runtime_failure;

#[path = "provider_request_broker.rs"]
mod provider_requests;
pub use provider_requests::{LiveProviderRequest, ResolvedProviderRequest};
use provider_requests::{RequestBroker, RequestCommand, RequestReceivers};

const ROOM_QUEUE_CAPACITY: usize = 128;
const ROOM_TOOL_QUEUE_CAPACITY: usize = 64;
const EVENT_RECEIVER_CAPACITY: usize = 256;
const PUBLICATION_WAKE_CAPACITY: usize = 128;
const ROOM_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
struct RoomHandle {
    mutations: mpsc::Sender<RoomMutation>,
    events: broadcast::Sender<RoomEvent>,
    human_session_revocations: broadcast::Sender<[u8; 32]>,
    publication_wake: mpsc::Sender<RoomPublicationWake>,
    provider_recovery: mpsc::Sender<RecoveredAssignments>,
    provider_requests: mpsc::Sender<RequestCommand>,
}

enum RoomPublicationWake {
    Publish,
    FinalizeDeletion(oneshot::Sender<Result<bool, PersistenceError>>),
}

struct RoomTaskContext {
    room_id: String,
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    provider_adapter: ProviderAdapter,
    cancellation: CancellationToken,
    event_tx: broadcast::Sender<RoomEvent>,
    human_session_revocation_tx: broadcast::Sender<[u8; 32]>,
    ingress: ProviderTurnIngress,
    lifecycle_commands: LifecycleCommandTracker,
    active_rooms: Arc<Mutex<HashMap<String, RoomHandle>>>,
}

struct RoomCommandOwners<'a> {
    store: &'a SqliteStore,
    provider_catalog: &'a ProviderCatalogService,
    provider_adapter: &'a ProviderAdapter,
    event_tx: &'a broadcast::Sender<RoomEvent>,
    turn_tasks: &'a mut JoinSet<ProviderTurnTaskResult>,
    ingress: &'a ProviderTurnIngress,
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
                    code: "room_busy".into(),
                    message: "Room mutation queue is full.".to_owned(),
                },
                mpsc::error::TrySendError::Closed(_) => PersistenceError::CommandRejected {
                    code: "room_unavailable".into(),
                    message: "Room mutation task stopped.".to_owned(),
                },
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandRejected {
                code: "room_unavailable".into(),
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

    pub(crate) async fn revoke_operator_pairing(
        &self,
        manager: &agentsassemble_persistence::LocalRoomManagerAuthority,
        pairing_id: uuid::Uuid,
    ) -> Result<(), PersistenceError> {
        let fingerprint = self
            .store
            .revoke_operator_pairing(manager, pairing_id)
            .await?;
        if let Some(fingerprint) = fingerprint {
            let rooms = self.rooms.lock().await;
            if let Some(handle) = rooms.get(&manager.manager.room_id) {
                let _ = handle.human_session_revocations.send(fingerprint);
            }
        }
        Ok(())
    }

    pub(crate) async fn recover_guest_identity(
        &self,
        request: &agentsassemble_persistence::GuestRecoveryRequest<'_>,
    ) -> Result<agentsassemble_persistence::GuestRecoveryCommit, PersistenceError> {
        let commit = self
            .store
            .redeem_guest_recovery_code(request, chrono::Utc::now())
            .await?;
        let rooms = self.rooms.lock().await;
        if let Some(handle) = rooms.get(&commit.result.meeting_id) {
            for fingerprint in &commit.replaced_session_fingerprints {
                let _ = handle.human_session_revocations.send(*fingerprint);
            }
        }
        Ok(commit)
    }

    pub async fn notify_committed_events(&self, events: &[RoomEvent]) {
        let mut notified_rooms = HashSet::new();
        for event in events {
            if !notified_rooms.insert(event.room_id.clone()) {
                continue;
            }
            let handle = self.handle(&event.room_id).await;
            let _ = handle
                .publication_wake
                .try_send(RoomPublicationWake::Publish);
        }
    }

    pub(crate) async fn notify_account_commit(
        &self,
        commit: &agentsassemble_persistence::GoogleAccountLink,
    ) {
        {
            let rooms = self.rooms.lock().await;
            for (room_id, fingerprint) in &commit.revoked_sessions {
                if let Some(handle) = rooms.get(room_id) {
                    let _ = handle.human_session_revocations.send(*fingerprint);
                }
            }
        }
        self.notify_committed_events(&commit.events).await;
    }

    pub(crate) async fn notify_room_publication(&self, room_id: &str) {
        let handle = self.handle(room_id).await;
        let _ = handle
            .publication_wake
            .try_send(RoomPublicationWake::Publish);
    }

    pub(crate) async fn finalize_room_deletion(
        &self,
        room_id: &str,
    ) -> Result<bool, PersistenceError> {
        let handle = self.handle(room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .publication_wake
            .try_send(RoomPublicationWake::FinalizeDeletion(reply))
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "room_busy".into(),
                message: "The room maintenance queue is unavailable; deletion remains pending."
                    .to_owned(),
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "room_unavailable".into(),
                message: "The deletion completion response was lost.".to_owned(),
            })?
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
                code: "provider_turn_recovery_authority_invalid".into(),
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
        let handle = self.handle(room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .provider_recovery
            .send(RecoveredAssignments { assignments, reply })
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_unavailable".into(),
                message: "The provider turn recovery owner stopped.".to_owned(),
            })?;
        response
            .await
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "provider_turn_recovery_unavailable".into(),
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
        self.handle_locked(room_id, &mut rooms).await
    }

    async fn handle_locked(
        &self,
        room_id: &str,
        rooms: &mut HashMap<String, RoomHandle>,
    ) -> RoomHandle {
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
        let (attachment_ingress, attachment_rx) =
            ProviderAttachmentReadIngress::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let (request_tx, request_rx) = mpsc::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let (request_ingress, native_requests) =
            agentsassemble_provider::ProviderRequestIngress::channel(ROOM_TOOL_QUEUE_CAPACITY);
        let handle = RoomHandle {
            mutations: mutation_tx,
            events: event_tx.clone(),
            human_session_revocations: human_session_revocation_tx.clone(),
            publication_wake: publication_tx,
            provider_recovery: provider_recovery_tx,
            provider_requests: request_tx,
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
                ingress: ProviderTurnIngress {
                    tools: room_tool_ingress,
                    attachments: attachment_ingress,
                    requests: request_ingress,
                },
                lifecycle_commands: self.lifecycle_commands.clone(),
                active_rooms: self.rooms.clone(),
            },
            mutation_rx,
            publication_rx,
            room_tool_rx,
            attachment_rx,
            provider_recovery_rx,
            RequestReceivers {
                browser: request_rx,
                native: native_requests,
            },
        );
        self.tasks.lock().await.push(task);
        handle
    }
}

fn spawn_room_task(
    context: RoomTaskContext,
    mut mutation_rx: mpsc::Receiver<RoomMutation>,
    mut publication_rx: mpsc::Receiver<RoomPublicationWake>,
    mut room_tool_rx: mpsc::Receiver<ProviderRoomToolCommand>,
    mut attachment_rx: mpsc::Receiver<ProviderAttachmentReadCommand>,
    mut provider_recovery_rx: mpsc::Receiver<RecoveredAssignments>,
    mut request_rx: RequestReceivers,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut turn_tasks = JoinSet::new();
        let mut requests = RequestBroker::new();
        let startup_publication =
            publish_durable_room_events(&context.store, &context.event_tx, &context.room_id).await;
        let mut publication_retry = PublicationRetry::new(startup_publication);
        let mut provider_write_budget = ProviderWriteBudget::new();
        loop {
            let input = tokio::select! {
                () = context.cancellation.cancelled() => {
                    abort_provider_turns(&mut turn_tasks).await;
                    break;
                }
                failure = context.provider_adapter.wait_for_runtime_failure(&context.room_id) => {
                    if let Err(error) = runtime_failure::record(
                        &context.store, &context.provider_adapter, &context.room_id, &failure,
                    ).await {
                        tracing::error!(%error, room_id = %context.room_id,
                            "managed runtime failure could not commit; room owner is stopping");
                        abort_provider_turns(&mut turn_tasks).await;
                        break;
                    }
                    RoomInput::Publication
                }
                request = request_rx.native.recv() => {
                    let Some(request) = request else { break; };
                    requests.apply_native(&context.store, &context.room_id, request).await;
                    RoomInput::Publication
                }
                request = request_rx.browser.recv() => {
                    let Some(request) = request else { break; };
                    requests.apply(&context.store, &context.room_id, request).await;
                    RoomInput::Publication
                }
                wake = requests.wake(), if requests.has_watches() => {
                    if let Some(wake) = wake
                        && requests.complete(&context.store, &context.room_id, wake).await.is_err()
                    {
                        tracing::error!(room_id = %context.room_id, "provider request completion failed; durable state remains authoritative");
                    }
                    RoomInput::Publication
                }
                mutation = mutation_rx.recv() => {
                    let Some(mutation) = mutation else { break; };
                    RoomInput::Mutation(mutation)
                }
                wake = publication_rx.recv() => {
                    match wake {
                        Some(RoomPublicationWake::Publish) => RoomInput::Publication,
                        Some(RoomPublicationWake::FinalizeDeletion(reply)) => {
                            if finalize_owned_room(&context, reply).await {
                                abort_provider_turns(&mut turn_tasks).await;
                                break;
                            }
                            continue;
                        }
                        None => break,
                    }
                }
                () = publication_retry.wait(), if publication_retry.is_armed() => RoomInput::Publication,
                result = turn_tasks.join_next(), if !turn_tasks.is_empty() => {
                    let Some(result) = result else { continue; };
                    RoomInput::Provider(Box::new(result))
                }
                tool = room_tool_rx.recv() => {
                    let Some(tool) = tool else { break; };
                    RoomInput::Tool(tool)
                }
                attachment = attachment_rx.recv() => {
                    let Some(attachment) = attachment else { break; };
                    RoomInput::Attachment(attachment)
                }
                recovery = provider_recovery_rx.recv() => {
                    let Some(assignment) = recovery else { break; };
                    RoomInput::ProviderRecovery(Box::new(assignment))
                }
            };
            let retry_exhausted =
                handle_room_input(&context, &mut turn_tasks, &mut provider_write_budget, input)
                    .await
                    .is_some_and(|publication| publication_retry.record(publication));
            if requests
                .reconcile(&context.store, &context.room_id)
                .await
                .is_err()
            {
                tracing::error!(room_id = %context.room_id, "provider request live reconciliation failed; room owner is stopping");
                abort_provider_turns(&mut turn_tasks).await;
                break;
            }
            if retry_exhausted {
                tracing::error!(
                    room_id = %context.room_id,
                    "automatic durable room-event publication retry exhausted; the durable backlog remains pending until a real room wake or restart"
                );
            }
        }
    })
}

async fn finalize_owned_room(
    context: &RoomTaskContext,
    reply: oneshot::Sender<Result<bool, PersistenceError>>,
) -> bool {
    if publish_durable_room_events(&context.store, &context.event_tx, &context.room_id).await
        == PublicationAttempt::Retry
    {
        let _ = reply.send(Ok(false));
        return false;
    }
    // The actor serializes commands with deletion. The existing map lock also
    // serializes physical deletion/retirement with immutable HTTP replay routing.
    let mut rooms = context.active_rooms.lock().await;
    let result = context.store.finish_room_deletion(&context.room_id).await;
    let retired = matches!(result, Ok(true));
    if retired {
        rooms.remove(&context.room_id);
    }
    let _ = reply.send(result);
    retired
}

async fn handle_room_input(
    context: &RoomTaskContext,
    turn_tasks: &mut JoinSet<ProviderTurnTaskResult>,
    provider_write_budget: &mut ProviderWriteBudget,
    input: RoomInput,
) -> Option<PublicationAttempt> {
    match input {
        RoomInput::Mutation(mutation) => {
            return handle_room_mutation(
                RoomCommandOwners {
                    store: &context.store,
                    provider_catalog: &context.provider_catalog,
                    provider_adapter: &context.provider_adapter,
                    event_tx: &context.event_tx,
                    turn_tasks,
                    ingress: &context.ingress,
                    lifecycle_commands: &context.lifecycle_commands,
                },
                &context.room_id,
                &context.human_session_revocation_tx,
                &context.active_rooms,
                mutation,
            )
            .await;
        }
        RoomInput::Provider(result) => {
            return handle_provider_result(
                &context.store,
                &context.provider_adapter,
                &context.event_tx,
                turn_tasks,
                *result,
                &context.ingress,
            )
            .await;
        }
        RoomInput::Tool(command) => {
            return crate::provider_room_tool_runtime::handle_provider_room_tool(
                &context.store,
                &context.event_tx,
                &context.room_id,
                command,
                provider_write_budget,
            )
            .await;
        }
        RoomInput::Attachment(command) => {
            crate::provider_attachment_runtime::handle_provider_attachment_read(
                &context.store,
                &context.room_id,
                command,
            )
            .await;
        }
        RoomInput::ProviderRecovery(recovery) => {
            return Some(
                RecoveryRuntime {
                    store: &context.store,
                    event_tx: &context.event_tx,
                    room_id: &context.room_id,
                    turn_tasks,
                    provider_adapter: &context.provider_adapter,
                    ingress: &context.ingress,
                }
                .publish_then_resume(*recovery)
                .await,
            );
        }
        RoomInput::Publication => {
            return Some(
                publish_durable_room_events(&context.store, &context.event_tx, &context.room_id)
                    .await,
            );
        }
    }
    None
}

async fn abort_provider_turns(turn_tasks: &mut JoinSet<ProviderTurnTaskResult>) {
    turn_tasks.abort_all();
    while turn_tasks.join_next().await.is_some() {}
}

async fn handle_room_mutation(
    owners: RoomCommandOwners<'_>,
    room_id: &str,
    session_revocations: &broadcast::Sender<[u8; 32]>,
    active_rooms: &Mutex<HashMap<String, RoomHandle>>,
    mutation: RoomMutation,
) -> Option<PublicationAttempt> {
    match mutation {
        RoomMutation::Command(command) => {
            Box::pin(handle_room_command(owners, session_revocations, command)).await
        }
        RoomMutation::ConnectorAdmission(command) => {
            connector::admit(&owners, room_id, command).await
        }
        RoomMutation::Attendee(command) => {
            Box::pin(attendee_execution::execute(owners, command)).await
        }
        RoomMutation::AttendeeAdmission(command) => {
            attendee::admit(&owners, room_id, command).await
        }
        RoomMutation::HumanAdmission(command) => {
            let publication = handle_human_admission(
                owners.store,
                room_id,
                owners.event_tx,
                session_revocations,
                command,
            )
            .await;
            notify_active_room_publications(active_rooms, publication.other_active_rooms).await;
            publication.current
        }
    }
}

async fn notify_active_room_publications(
    active_rooms: &Mutex<HashMap<String, RoomHandle>>,
    room_ids: HashSet<String>,
) {
    let wakes = {
        let rooms = active_rooms.lock().await;
        room_ids
            .into_iter()
            .filter_map(|room_id| {
                rooms
                    .get(&room_id)
                    .map(|room| room.publication_wake.clone())
            })
            .collect::<Vec<_>>()
    };
    for wake in wakes {
        let _ = wake.try_send(RoomPublicationWake::Publish);
    }
}

async fn handle_room_command(
    owners: RoomCommandOwners<'_>,
    session_revocations: &broadcast::Sender<[u8; 32]>,
    command: RoomCommand,
) -> Option<PublicationAttempt> {
    let RoomCommandOwners {
        store,
        provider_catalog,
        provider_adapter,
        event_tx,
        turn_tasks,
        ingress,
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
            code: "runtime_recovery_in_progress".into(),
            message: "The exact lifecycle request is currently owned by server recovery. Retry the same request.".to_owned(),
        }),
        Some(_lifecycle_guard) => {
            let incarnation = match command.room_uid {
                Some(expected) => store.require_room_incarnation(&command.principal.room_id, expected).await,
                None => Ok(()),
            };
            if let Err(error) = incarnation {
                CommandExecution::transactional_failure(error)
            } else {
                Box::pin(crate::room_command_dispatch::execute_command(
                store,
                provider_catalog,
                provider_adapter,
                event_tx,
                &command,
            ))
            .await
            }
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
    let publication = if committed_events.is_empty() {
        None
    } else {
        Some(publish_durable_room_events(store, event_tx, &command.principal.room_id).await)
    };
    for assignment in assignments {
        spawn_provider_turn(
            turn_tasks,
            store.clone(),
            provider_adapter.clone(),
            assignment,
            ingress.clone(),
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
    publication
}

enum RoomInput {
    Mutation(RoomMutation),
    Provider(Box<Result<ProviderTurnTaskResult, tokio::task::JoinError>>),
    Publication,
    Tool(ProviderRoomToolCommand),
    Attachment(ProviderAttachmentReadCommand),
    ProviderRecovery(Box<RecoveredAssignments>),
}

enum RoomMutation {
    Command(RoomCommand),
    HumanAdmission(HumanAdmissionCommand),
    ConnectorAdmission(connector::ConnectorAdmissionCommand),
    AttendeeAdmission(attendee::AttendeeAdmissionCommand),
    Attendee(attendee_execution::AttendeeCommand),
}
