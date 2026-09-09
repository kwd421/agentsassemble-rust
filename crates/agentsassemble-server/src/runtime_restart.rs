use std::{path::PathBuf, sync::Arc};

use agentsassemble_domain::{RuntimeRestartReceipt, RuntimeRestartStatus};
use agentsassemble_persistence::{PersistenceError, RuntimeRestartRecord, SqliteStore};
use tokio::sync::{Mutex, oneshot};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{AppState, frontend_release::FrontendRelease, runtime_image::RuntimeImage};

pub struct PreparedRestart {
    pub operation_id: String,
    pub image: RuntimeImage,
    pub frontend_build: Option<String>,
}

#[derive(Clone, Default)]
pub struct RuntimeRestartControl {
    inner: Option<Arc<RestartOwner>>,
}

struct RestartOwner {
    executable: PathBuf,
    frontend: Option<PathBuf>,
    state_root: PathBuf,
    cancellation: CancellationToken,
    delivery: Mutex<Option<oneshot::Sender<PreparedRestart>>>,
}

#[derive(Debug, thiserror::Error)]
pub enum RuntimeRestartError {
    #[error("rolling restart is unavailable for this runtime")]
    Unavailable,
    #[error("another runtime restart request is being prepared")]
    Busy,
    #[error("runtime replacement preparation failed")]
    Candidate,
    #[error("runtime restart state could not be updated")]
    Persistence(#[from] PersistenceError),
}

impl RuntimeRestartControl {
    #[must_use]
    pub fn new(
        executable: PathBuf,
        frontend: Option<PathBuf>,
        state_root: PathBuf,
        cancellation: CancellationToken,
    ) -> (Self, oneshot::Receiver<PreparedRestart>) {
        let (sender, receiver) = oneshot::channel();
        (
            Self {
                inner: Some(Arc::new(RestartOwner {
                    executable,
                    frontend,
                    state_root,
                    cancellation,
                    delivery: Mutex::new(Some(sender)),
                })),
            },
            receiver,
        )
    }

    /// Reads a durable receipt without treating a lost connection as completion.
    ///
    /// # Errors
    /// Returns storage and malformed-state failures.
    pub async fn status(
        &self,
        store: &SqliteStore,
        operation: Option<Uuid>,
    ) -> Result<RuntimeRestartStatus, PersistenceError> {
        let record = match operation {
            Some(id) => store.runtime_restart_operation(&id.to_string()).await?,
            None => store.runtime_restart_status().await?,
        };
        Ok(RuntimeRestartStatus {
            supported: cfg!(unix) && self.inner.is_some(),
            operation: record.map(receipt),
        })
    }

    /// Prepares one actual replacement and transfers its effect to the main process owner.
    ///
    /// # Errors
    /// Rejects unsupported/busy execution, invalid candidates and durable admission failures.
    pub async fn request(
        &self,
        state: &AppState,
        operation: Uuid,
    ) -> Result<RuntimeRestartReceipt, RuntimeRestartError> {
        let owner = self
            .inner
            .as_ref()
            .filter(|_| cfg!(unix))
            .ok_or(RuntimeRestartError::Unavailable)?;
        let operation_id = operation.to_string();
        if let Some(previous) = state.store.runtime_restart_operation(&operation_id).await? {
            return Ok(receipt(previous));
        }
        let mut delivery = owner
            .delivery
            .try_lock()
            .map_err(|_| RuntimeRestartError::Busy)?;
        if delivery.is_none() {
            return Err(RuntimeRestartError::Busy);
        }
        let image = RuntimeImage::prepare(&owner.executable, &owner.state_root)
            .await
            .map_err(|_| RuntimeRestartError::Candidate)?;
        let frontend_build = if let Some(source) = owner.frontend.clone() {
            let root = owner.state_root.clone();
            Some(
                tokio::task::spawn_blocking(move || FrontendRelease::materialize(&source, &root))
                    .await
                    .map_err(|_| RuntimeRestartError::Candidate)?
                    .map_err(|_| RuntimeRestartError::Candidate)?
                    .build_id()
                    .to_owned(),
            )
        } else {
            None
        };
        let prepared = state.store.prepare_runtime_restart(&operation_id).await?;
        if prepared.phase != agentsassemble_domain::RuntimeRestartPhase::Quiescing {
            return Ok(receipt(prepared));
        }
        let sender = delivery.take().ok_or(RuntimeRestartError::Unavailable)?;
        let sent = sender.send(PreparedRestart {
            operation_id,
            image,
            frontend_build,
        });
        owner.cancellation.cancel();
        if sent.is_err() {
            state
                .store
                .abort_runtime_restart(&prepared.operation_id)
                .await?;
            return Err(RuntimeRestartError::Unavailable);
        }
        Ok(receipt(prepared))
    }
}

fn receipt(record: RuntimeRestartRecord) -> RuntimeRestartReceipt {
    RuntimeRestartReceipt {
        operation_id: record.operation_id,
        phase: record.phase,
        updated_at: record.updated_at,
    }
}

/// Reconstructs exact captured sessions through the real provider supervisor before readiness.
///
/// # Errors
/// Returns unresolved provider custody and failed durable confirmations. The serving owner
/// must finish ordinary positive cleanup before it may release the failed operation's barrier.
pub async fn recover(
    state: &AppState,
    operation_id: &str,
    candidate_identity: &str,
) -> anyhow::Result<()> {
    let record = state
        .store
        .begin_runtime_restart_recovery(operation_id, candidate_identity)
        .await?;
    for target in record.targets {
        let session = state
            .store
            .runtime_restart_target(operation_id, &target.room_id, &target.session_id)
            .await?;
        let reservation = state.provider_adapter.reserve_start(&session).await?;
        let authorized = state
            .store
            .authorize_runtime_restart_target(
                operation_id,
                &target,
                &reservation.runtime_handle_id,
                &reservation.runtime_owner_id,
                &reservation.runtime_lease_token,
            )
            .await;
        let authorized = match authorized {
            Ok(session) => session,
            Err(error) => {
                state
                    .provider_adapter
                    .cancel_start_reservation(&target.room_id, &target.session_id, &reservation)
                    .await;
                return Err(error.into());
            }
        };
        let started = state.provider_adapter.start_reserved(&authorized).await?;
        state
            .store
            .complete_runtime_restart_target(
                operation_id,
                &target.room_id,
                &target.session_id,
                &agentsassemble_persistence::AgentRuntimeStarted {
                    runtime_handle_id: started.runtime_handle_id,
                    runtime_owner_id: started.runtime_owner_id,
                    runtime_lease_token: started.runtime_lease_token,
                    provider_session_id: started.provider_session_id,
                    runtime_reused: started.runtime_reused,
                    provider_session_reused: started.provider_session_reused,
                    provider_session_active: started.provider_session_active,
                },
            )
            .await?;
    }
    state.store.complete_runtime_restart(operation_id).await?;
    wake_floor(state).await?;
    Ok(())
}

async fn wake_floor(state: &AppState) -> Result<(), PersistenceError> {
    for summary in state.store.list_room_directory(false).await? {
        let room = summary.room;
        if room.status != agentsassemble_domain::RoomStatus::Active
            || summary.cleanup_pending
            || summary.deletion_pending
        {
            continue;
        }
        if let Some(assignment) = state.store.assign_pending_turn(&room.room_id).await? {
            state
                .rooms
                .publish_then_resume_assigned_turns(&room.room_id, assignment.next_assignments)
                .await?;
        }
    }
    Ok(())
}
