use std::{collections::HashMap, sync::Arc};

use agentsassemble_domain::{AuthenticatedPrincipal, RoomEvent};
use agentsassemble_persistence::{CommandOutcome, PersistenceError, SqliteStore};
use serde_json::Value;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot};

const ROOM_QUEUE_CAPACITY: usize = 128;
const EVENT_RECEIVER_CAPACITY: usize = 256;

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
    rooms: Arc<Mutex<HashMap<String, RoomHandle>>>,
}

impl RoomRuntime {
    #[must_use]
    pub fn new(store: SqliteStore) -> Self {
        Self {
            store,
            rooms: Arc::new(Mutex::new(HashMap::new())),
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
            .send(RoomCommand {
                principal,
                request_id,
                action,
                payload,
                reply,
            })
            .await
            .map_err(|_| PersistenceError::CommandRejected {
                code: "room_unavailable",
                message: "Room mutation task stopped.".to_owned(),
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
        tokio::spawn(async move {
            while let Some(command) = command_rx.recv().await {
                let result = store
                    .execute_message(
                        &command.principal,
                        &command.request_id,
                        &command.action,
                        &command.payload,
                    )
                    .await;
                if let Ok(outcome) = &result
                    && !outcome.deduplicated
                {
                    let _ = event_tx.send(outcome.event.clone());
                }
                let _ = command.reply.send(result);
            }
        });
        handle
    }
}
