//! The local server retains each attendee's identity and task across frontend request loss.
use std::{collections::HashMap, sync::Arc};

use agentsassemble_domain::{
    LocalAttendeeCreate, LocalAttendeePhase as Phase, LocalAttendeeStatus,
};
use agentsassemble_persistence::SqliteStore;
use agentsassemble_provider::{ProviderAdapter, ProviderCatalogService, creation_start_requested};
use futures_util::{
    FutureExt,
    future::{BoxFuture, Shared},
};
use tokio::sync::{Mutex, mpsc, watch};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{AttendeeClientError, AttendeeJoined, AttendeeRuntime, RoomAttendeeClient};

#[path = "local_attendee_session.rs"]
mod session;

#[derive(Debug, Clone, thiserror::Error)]
#[error("{code}")]
pub struct LocalAttendeeError {
    pub code: String,
}

impl LocalAttendeeError {
    fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
        }
    }
}

impl From<AttendeeClientError> for LocalAttendeeError {
    fn from(error: AttendeeClientError) -> Self {
        Self { code: error.code }
    }
}

#[derive(Clone)]
pub struct LocalAttendeeService {
    operations: Arc<Mutex<HashMap<Uuid, Arc<Operation>>>>,
    shutdown: CancellationToken,
}

struct Operation {
    request: LocalAttendeeCreate,
    invitation_identity: [u8; 32],
    status: watch::Receiver<LocalAttendeeStatus>,
    commands: mpsc::Sender<Advance>,
    cancellation: CancellationToken,
    task: Shared<BoxFuture<'static, Result<(), LocalAttendeeError>>>,
    // A completed/failed task is not permission to discard uncertain client or process custody.
    _custody: Arc<Mutex<Custody>>,
}

pub(super) enum Advance {
    RetryAdmission,
    Start,
}

struct Custody {
    client: RoomAttendeeClient,
    joined: Option<AttendeeJoined>,
    runtime: Option<AttendeeRuntime>,
}

impl LocalAttendeeService {
    #[must_use]
    pub fn new(shutdown: CancellationToken) -> Self {
        Self {
            operations: Arc::default(),
            shutdown,
        }
    }

    /// Starts one local creation owner; dropped waiters cannot cancel its external effects.
    ///
    /// # Errors
    /// Rejects changed retries, duplicate invitations, invalid local input and closed service.
    pub async fn create(
        &self,
        request: LocalAttendeeCreate,
        catalog: ProviderCatalogService,
        store: SqliteStore,
        adapter: ProviderAdapter,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        let operation = {
            let mut operations = self.operations.lock().await;
            if self.shutdown.is_cancelled() {
                return Err(LocalAttendeeError::new("local_attendee_closed"));
            }
            if let Some(operation) = operations.get(&request.request_id) {
                if operation.request != request {
                    return Err(LocalAttendeeError::new("local_attendee_request_changed"));
                }
                operation.clone()
            } else {
                let start = creation_start_requested(&request.creation)
                    .map_err(|error| LocalAttendeeError::new(error.code))?;
                if request.request_id.is_nil() {
                    return Err(LocalAttendeeError::new("invalid_local_attendee_request"));
                }
                let provider = request
                    .creation
                    .get("provider_id")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| LocalAttendeeError::new("invalid_local_attendee_request"))?;
                let name = request
                    .creation
                    .get("display_name")
                    .and_then(serde_json::Value::as_str)
                    .ok_or_else(|| LocalAttendeeError::new("invalid_local_attendee_request"))?;
                let client = RoomAttendeeClient::for_room(
                    &request.invite_url,
                    provider,
                    name,
                    request.room_id.clone(),
                    request.room_uid,
                )?;
                if operations
                    .values()
                    .any(|operation| operation.invitation_identity == client.invitation_identity())
                {
                    return Err(LocalAttendeeError::new("local_attendee_invitation_owned"));
                }
                let operation = self.spawn(request, client, start, catalog, store, adapter);
                operations.insert(operation.request.request_id, operation.clone());
                operation
            }
        };
        operation.observe_settled().await
    }

    fn spawn(
        &self,
        request: LocalAttendeeCreate,
        client: RoomAttendeeClient,
        start: bool,
        catalog: ProviderCatalogService,
        store: SqliteStore,
        adapter: ProviderAdapter,
    ) -> Arc<Operation> {
        let invitation_identity = client.invitation_identity();
        let (status, observation) = watch::channel(LocalAttendeeStatus {
            request_id: request.request_id,
            room_id: request.room_id.clone(),
            room_uid: request.room_uid,
            participant_id: None,
            phase: Phase::Admitting,
            error_code: None,
        });
        let (commands, receiver) = mpsc::channel(1);
        let cancellation = self.shutdown.child_token();
        let custody = Arc::new(Mutex::new(Custody {
            client,
            joined: None,
            runtime: None,
        }));
        let owned = custody.clone();
        let input = session::Input {
            request: request.clone(),
            start,
            catalog,
            store,
            adapter,
            status: status.clone(),
            commands: receiver,
            cancellation: cancellation.clone(),
        };
        let task = tokio::spawn(async move {
            let outcome = Box::pin(session::run(&mut *owned.lock().await, input)).await;
            if let Err(error) = &outcome {
                status.send_modify(|state| {
                    state.phase = Phase::CleanupUnconfirmed;
                    state.error_code = Some(error.code.clone());
                });
            }
            outcome
        });
        let task = async move {
            task.await
                .map_err(|_| LocalAttendeeError::new("local_attendee_task_unresolved"))?
        }
        .boxed()
        .shared();
        Arc::new(Operation {
            request,
            invitation_identity,
            status: observation,
            commands,
            cancellation,
            task,
            _custody: custody,
        })
    }

    /// Reads the retained result without admission, launch, or retry effects.
    ///
    /// # Errors
    /// Rejects an unknown operation identity.
    pub async fn status(
        &self,
        request_id: Uuid,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        Operation::snapshot(&self.operation(request_id).await?.status)
    }

    /// Retries only an uncertain admission, using its original secret and request identity.
    ///
    /// # Errors
    /// Rejects an operation that is not awaiting admission recovery.
    pub async fn retry(&self, request_id: Uuid) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        self.advance(
            request_id,
            Phase::AdmissionUnresolved,
            Advance::RetryAdmission,
        )
        .await
    }

    /// Starts the already admitted local draft, without creating another room participant.
    ///
    /// # Errors
    /// Rejects an operation that is not admitted and awaiting a local start.
    pub async fn start(&self, request_id: Uuid) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        self.advance(request_id, Phase::Admitted, Advance::Start)
            .await
    }

    async fn advance(
        &self,
        id: Uuid,
        phase: Phase,
        command: Advance,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        let operation = self.operation(id).await?;
        let mut observation = operation.status.clone();
        if observation.borrow_and_update().phase != phase {
            return Err(LocalAttendeeError::new("local_attendee_action_unavailable"));
        }
        operation
            .commands
            .try_send(command)
            .map_err(|_| LocalAttendeeError::new("local_attendee_action_pending"))?;
        observation
            .changed()
            .await
            .map_err(|_| LocalAttendeeError::new("local_attendee_task_unresolved"))?;
        Operation::settled(observation).await
    }

    /// Cancels and joins the retained task; uncertain cleanup remains a retained failure.
    ///
    /// # Errors
    /// Rejects unknown operations or unconfirmed admission/process/remote cleanup.
    pub async fn cancel(
        &self,
        request_id: Uuid,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        let operation = self.operation(request_id).await?;
        operation.cancellation.cancel();
        operation.task.clone().await?;
        Ok(operation.status.borrow().clone())
    }

    async fn operation(&self, id: Uuid) -> Result<Arc<Operation>, LocalAttendeeError> {
        self.operations
            .lock()
            .await
            .get(&id)
            .cloned()
            .ok_or_else(|| LocalAttendeeError::new("local_attendee_missing"))
    }

    /// Joins every owner before reporting the first retained cleanup failure.
    ///
    /// # Errors
    /// Unconfirmed effects and task loss remain failures on concurrent or repeated shutdown.
    pub async fn shutdown(&self) -> Result<(), LocalAttendeeError> {
        self.shutdown.cancel();
        let tasks = self
            .operations
            .lock()
            .await
            .values()
            .map(|operation| operation.task.clone())
            .collect::<Vec<_>>();
        let outcomes = futures_util::future::join_all(tasks).await;
        for outcome in outcomes {
            outcome?;
        }
        Ok(())
    }
}

impl Operation {
    fn snapshot(
        status: &watch::Receiver<LocalAttendeeStatus>,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        let value = status.borrow();
        if status.has_changed().is_err()
            && !matches!(
                value.phase,
                Phase::Stopped | Phase::Failed | Phase::CleanupUnconfirmed
            )
        {
            return Err(LocalAttendeeError::new("local_attendee_task_unresolved"));
        }
        Ok(value.clone())
    }

    async fn observe_settled(&self) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        Self::settled(self.status.clone()).await
    }

    async fn settled(
        mut status: watch::Receiver<LocalAttendeeStatus>,
    ) -> Result<LocalAttendeeStatus, LocalAttendeeError> {
        loop {
            status.borrow_and_update();
            let value = Self::snapshot(&status)?;
            if !matches!(
                value.phase,
                Phase::Admitting | Phase::Starting | Phase::Stopping
            ) {
                return Ok(value);
            }
            status
                .changed()
                .await
                .map_err(|_| LocalAttendeeError::new("local_attendee_task_unresolved"))?;
        }
    }
}
