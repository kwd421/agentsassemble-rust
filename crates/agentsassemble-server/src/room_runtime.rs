use std::{collections::HashMap, sync::Arc, time::Duration};

use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{CommandOutcome, PersistenceError, SqliteStore};
use agentsassemble_provider::ProviderCatalogService;
use serde_json::Value;
use thiserror::Error;
use tokio::{
    sync::{Mutex, broadcast, mpsc, oneshot},
    task::JoinHandle,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

const ROOM_QUEUE_CAPACITY: usize = 128;
const EVENT_RECEIVER_CAPACITY: usize = 256;
const ROOM_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RoomShutdownError {
    #[error("room mutation tasks exceeded the shutdown deadline")]
    TimedOut,
    #[error("room mutation task failed: {0}")]
    TaskFailed(String),
}

struct RoomCommand {
    principal: AuthenticatedPrincipal,
    request_id: String,
    action: String,
    payload: Value,
    reply: oneshot::Sender<Result<CommandOutcome, PersistenceError>>,
}

#[derive(Clone)]
struct RoomHandle {
    commands: mpsc::Sender<RoomCommand>,
    events: broadcast::Sender<RoomEvent>,
}

#[derive(Clone)]
pub struct RoomRuntime {
    store: SqliteStore,
    provider_catalog: ProviderCatalogService,
    rooms: Arc<Mutex<HashMap<String, RoomHandle>>>,
    cancellation: CancellationToken,
    tasks: Arc<Mutex<Vec<JoinHandle<()>>>>,
}

impl RoomRuntime {
    #[must_use]
    pub fn new(store: SqliteStore, provider_catalog: ProviderCatalogService) -> Self {
        Self {
            store,
            provider_catalog,
            rooms: Arc::new(Mutex::new(HashMap::new())),
            cancellation: CancellationToken::new(),
            tasks: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Enqueues one durable command on its room's single mutation task.
    ///
    /// # Errors
    ///
    /// Returns the command or persistence failure, including a stopped room task.
    pub async fn execute(
        &self,
        principal: AuthenticatedPrincipal,
        request_id: String,
        action: String,
        payload: Value,
    ) -> Result<CommandOutcome, PersistenceError> {
        let handle = self.handle(&principal.room_id).await;
        let (reply, response) = oneshot::channel();
        handle
            .commands
            .try_send(RoomCommand {
                principal,
                request_id,
                action,
                payload,
                reply,
            })
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => PersistenceError::CommandRejected {
                    code: "room_busy",
                    message: "Room command queue is full.".to_owned(),
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
                message: "Room mutation response was lost.".to_owned(),
            })?
    }

    pub async fn subscribe(&self, room_id: &str) -> broadcast::Receiver<RoomEvent> {
        self.handle(room_id).await.events.subscribe()
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
        join_room_tasks(tasks, ROOM_SHUTDOWN_TIMEOUT).await
    }

    async fn handle(&self, room_id: &str) -> RoomHandle {
        let mut rooms = self.rooms.lock().await;
        if let Some(handle) = rooms.get(room_id) {
            return handle.clone();
        }
        let (command_tx, mut command_rx) = mpsc::channel::<RoomCommand>(ROOM_QUEUE_CAPACITY);
        let (event_tx, _) = broadcast::channel(EVENT_RECEIVER_CAPACITY);
        let handle = RoomHandle {
            commands: command_tx,
            events: event_tx.clone(),
        };
        rooms.insert(room_id.to_owned(), handle.clone());
        let store = self.store.clone();
        let provider_catalog = self.provider_catalog.clone();
        let cancellation = self.cancellation.clone();
        let task = tokio::spawn(async move {
            loop {
                let command = tokio::select! {
                    () = cancellation.cancelled() => break,
                    command = command_rx.recv() => {
                        let Some(command) = command else { break; };
                        command
                    }
                };
                let result = execute_command(&store, &provider_catalog, &command).await;
                if let Ok(outcome) = &result
                    && !outcome.deduplicated
                {
                    let _ = event_tx.send(outcome.event.clone());
                }
                let _ = command.reply.send(result);
            }
        });
        self.tasks.lock().await.push(task);
        handle
    }
}

async fn join_room_tasks(
    tasks: Vec<JoinHandle<()>>,
    timeout: Duration,
) -> Result<(), RoomShutdownError> {
    let deadline = Instant::now() + timeout;
    let mut failure = None;
    for mut task in tasks {
        match tokio::time::timeout_at(deadline, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                failure.get_or_insert_with(|| RoomShutdownError::TaskFailed(error.to_string()));
            }
            Err(_) => {
                task.abort();
                let _ = task.await;
                failure.get_or_insert(RoomShutdownError::TimedOut);
            }
        }
    }
    failure.map_or(Ok(()), Err)
}

async fn execute_command(
    store: &SqliteStore,
    provider_catalog: &ProviderCatalogService,
    command: &RoomCommand,
) -> Result<CommandOutcome, PersistenceError> {
    if command.action != "agent.create" {
        return store
            .execute_message(
                &command.principal,
                &command.request_id,
                &command.action,
                &command.payload,
            )
            .await;
    }
    if let Some(outcome) = store
        .replay_command(
            &command.principal,
            &command.request_id,
            &command.action,
            &command.payload,
        )
        .await?
    {
        return Ok(outcome);
    }
    let selection = provider_catalog
        .validate_creation(
            &command.principal.room_id,
            &command.principal.principal_id,
            &command.request_id,
            &command.payload,
        )
        .await
        .map_err(|error| PersistenceError::CommandRejected {
            code: error.code,
            message: error.message,
        })?;
    store
        .execute_agent_create(
            &command.principal,
            &command.request_id,
            &command.payload,
            &selection.into(),
        )
        .await
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{RoomShutdownError, join_room_tasks};

    #[tokio::test]
    async fn stalled_room_task_is_aborted_within_one_deadline() {
        let task = tokio::spawn(std::future::pending::<()>());
        let result = join_room_tasks(vec![task], Duration::from_millis(10)).await;
        assert_eq!(result, Err(RoomShutdownError::TimedOut));
    }
}
