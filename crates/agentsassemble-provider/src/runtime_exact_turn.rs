use std::time::Duration;

use agentsassemble_domain::DurableAgentSession;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use super::{
    DriverError, ProviderAdapter, ProviderAdapterError, ProviderTurnRequest, RuntimeState,
    validate_owned_runtime,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveProviderTurnPhase {
    Preparing,
    Entered,
    QuiescedInterruptedRuntimeRetained,
    QuiescedOther,
}

pub(super) struct ActiveProviderTurnSlot {
    preparation_id: Uuid,
    execution_id: String,
    turn_id: String,
    turn_generation: u64,
    interruption: CancellationToken,
    phase: ActiveProviderTurnPhase,
    completion: watch::Sender<ActiveProviderTurnPhase>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderPreparedTurn {
    room_id: String,
    session_id: String,
    execution_id: String,
    turn_id: String,
    turn_generation: u64,
    runtime_handle_id: String,
    runtime_owner_id: String,
    runtime_lease_token: String,
    preparation_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderExactTurnAuthority {
    pub room_id: String,
    pub session_id: String,
    pub execution_id: String,
    pub turn_id: String,
    pub turn_generation: u64,
    pub runtime_handle_id: String,
    pub runtime_owner_id: String,
    pub runtime_lease_token: String,
}

impl ProviderPreparedTurn {
    #[must_use]
    pub fn exact_authority(&self) -> ProviderExactTurnAuthority {
        ProviderExactTurnAuthority {
            room_id: self.room_id.clone(),
            session_id: self.session_id.clone(),
            execution_id: self.execution_id.clone(),
            turn_id: self.turn_id.clone(),
            turn_generation: self.turn_generation,
            runtime_handle_id: self.runtime_handle_id.clone(),
            runtime_owner_id: self.runtime_owner_id.clone(),
            runtime_lease_token: self.runtime_lease_token.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderTurnInterruptDisposition {
    NotStarted,
    Started,
}

#[derive(Debug, Clone)]
pub struct ProviderTurnControl {
    pub disposition: ProviderTurnInterruptDisposition,
    interruption: CancellationToken,
    completion: watch::Receiver<ActiveProviderTurnPhase>,
}

impl ProviderTurnControl {
    /// Signals the exact turn owner after durable effect dispatch authorization.
    pub fn request_interrupt(&self) {
        self.interruption.cancel();
    }

    /// Waits for the exact turn owner to return its driver and close `RoomPortal` admission.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed error if the owner cannot prove quiescence in time.
    pub async fn wait_quiesced(&mut self, timeout: Duration) -> Result<(), ProviderAdapterError> {
        tokio::time::timeout(timeout, async {
            loop {
                match *self.completion.borrow_and_update() {
                    ActiveProviderTurnPhase::QuiescedInterruptedRuntimeRetained => return Ok(()),
                    ActiveProviderTurnPhase::QuiescedOther => {
                        return Err(ProviderAdapterError::safe(DriverError::new(
                            "provider_turn_interrupt_unconfirmed",
                            "The exact provider turn quiesced without confirmed retained-runtime interruption.",
                        )));
                    }
                    ActiveProviderTurnPhase::Preparing | ActiveProviderTurnPhase::Entered => {}
                }
                self.completion.changed().await.map_err(|_| {
                    ProviderAdapterError::safe(DriverError::new(
                        "provider_turn_quiescence_unconfirmed",
                        "The exact provider turn owner ended without quiescence proof.",
                    ))
                })?;
            }
        })
        .await
        .map_err(|_| {
            ProviderAdapterError::safe(DriverError::new(
                "provider_turn_quiescence_timeout",
                "The exact provider turn did not quiesce before its deadline.",
            ))
        })?
    }
}

impl ProviderAdapter {
    /// Installs exact in-memory turn authority before durable start authorization.
    ///
    /// # Errors
    ///
    /// Rejects stale runtime custody, malformed assignment, or another active turn.
    pub async fn prepare_turn(
        &self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderPreparedTurn, ProviderAdapterError> {
        super::turn::validate_request(session, request).map_err(ProviderAdapterError::safe)?;
        loop {
            let slot = self
                .existing_slot(&session.public.room_id, &session.public.session_id)
                .await
                .ok_or_else(|| owner_mismatch(session))?;
            let mut slot = slot.lock().await;
            let RuntimeState::Running(runtime) = &mut slot.state else {
                return Err(owner_mismatch(session));
            };
            validate_owned_runtime(session, runtime)?;
            if let Some(active) = &runtime.active_turn {
                if !same_execution(active, request) {
                    return Err(ProviderAdapterError::safe(operation_in_progress()));
                }
                if active.phase == ActiveProviderTurnPhase::Preparing {
                    return Ok(prepared_from_active(session, runtime, active));
                }
                let mut completion = active.completion.subscribe();
                drop(slot);
                loop {
                    if matches!(
                        *completion.borrow_and_update(),
                        ActiveProviderTurnPhase::QuiescedInterruptedRuntimeRetained
                            | ActiveProviderTurnPhase::QuiescedOther
                    ) {
                        break;
                    }
                    completion.changed().await.map_err(|_| {
                        ProviderAdapterError::safe(DriverError::new(
                            "provider_turn_quiescence_unconfirmed",
                            "The exact provider turn owner ended without quiescence proof.",
                        ))
                    })?;
                }
                continue;
            }
            let driver = runtime
                .driver
                .try_take()
                .map_err(ProviderAdapterError::safe)?;
            runtime.driver.put(driver).await;
            let preparation_id = Uuid::new_v4();
            let (completion, _) = watch::channel(ActiveProviderTurnPhase::Preparing);
            runtime.active_turn = Some(ActiveProviderTurnSlot {
                preparation_id,
                execution_id: request.execution_id.clone(),
                turn_id: request.turn_id.clone(),
                turn_generation: request.turn_generation,
                interruption: CancellationToken::new(),
                phase: ActiveProviderTurnPhase::Preparing,
                completion,
            });
            return Ok(ProviderPreparedTurn {
                room_id: session.public.room_id.clone(),
                session_id: session.public.session_id.clone(),
                execution_id: request.execution_id.clone(),
                turn_id: request.turn_id.clone(),
                turn_generation: request.turn_generation,
                runtime_handle_id: runtime.handle_id.clone(),
                runtime_owner_id: runtime.owner_id.clone(),
                runtime_lease_token: runtime.lease_token.clone(),
                preparation_id,
            });
        }
    }

    pub(super) async fn enter_prepared_turn(
        &self,
        prepared: &ProviderPreparedTurn,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<CancellationToken, ProviderAdapterError> {
        let slot = self
            .existing_slot(&prepared.room_id, &prepared.session_id)
            .await
            .ok_or_else(|| owner_mismatch(session))?;
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return Err(owner_mismatch(session));
        };
        validate_prepared(runtime, prepared, session, request)?;
        let active = runtime
            .active_turn
            .as_mut()
            .ok_or_else(|| ProviderAdapterError::safe(stale_turn()))?;
        active.phase = ActiveProviderTurnPhase::Entered;
        active
            .completion
            .send_replace(ActiveProviderTurnPhase::Entered);
        Ok(active.interruption.clone())
    }

    /// Removes a preparation whose durable start authorization was not consumed.
    pub async fn discard_prepared_turn(&self, prepared: &ProviderPreparedTurn) {
        let Some(slot) = self
            .existing_slot(&prepared.room_id, &prepared.session_id)
            .await
        else {
            return;
        };
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return;
        };
        if runtime.active_turn.as_ref().is_some_and(|active| {
            active.preparation_id == prepared.preparation_id
                && active.phase == ActiveProviderTurnPhase::Preparing
        }) && let Some(active) = runtime.active_turn.take()
        {
            let phase = if active.interruption.is_cancelled() {
                ActiveProviderTurnPhase::QuiescedInterruptedRuntimeRetained
            } else {
                ActiveProviderTurnPhase::QuiescedOther
            };
            active.completion.send_replace(phase);
        }
    }

    pub(super) async fn quiesce_prepared_turn(
        &self,
        prepared: &ProviderPreparedTurn,
        interrupted_runtime_retained: bool,
    ) {
        let Some(slot) = self
            .existing_slot(&prepared.room_id, &prepared.session_id)
            .await
        else {
            return;
        };
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return;
        };
        if runtime
            .active_turn
            .as_ref()
            .is_some_and(|active| active.preparation_id == prepared.preparation_id)
            && let Some(active) = runtime.active_turn.take()
        {
            let phase = if interrupted_runtime_retained {
                ActiveProviderTurnPhase::QuiescedInterruptedRuntimeRetained
            } else {
                ActiveProviderTurnPhase::QuiescedOther
            };
            active.completion.send_replace(phase);
        }
    }

    /// Resolves one exact prepared or entered turn without issuing provider control.
    ///
    /// # Errors
    ///
    /// Rejects stale generation or runtime custody rather than targeting current work.
    /// The caller must durably authorize dispatch before signaling a started turn.
    pub async fn begin_exact_turn(
        &self,
        authority: &ProviderExactTurnAuthority,
    ) -> Result<ProviderTurnControl, ProviderAdapterError> {
        let slot = self
            .existing_slot(&authority.room_id, &authority.session_id)
            .await
            .ok_or_else(|| ProviderAdapterError::safe(stale_turn()))?;
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return Err(ProviderAdapterError::safe(stale_turn()));
        };
        if runtime.handle_id != authority.runtime_handle_id
            || runtime.owner_id != authority.runtime_owner_id
            || runtime.lease_token != authority.runtime_lease_token
        {
            return Err(ProviderAdapterError::safe(stale_turn()));
        }
        let active = runtime
            .active_turn
            .as_mut()
            .filter(|active| exact_authority_matches(active, authority))
            .ok_or_else(|| ProviderAdapterError::safe(stale_turn()))?;
        let disposition = if active.phase == ActiveProviderTurnPhase::Preparing {
            ProviderTurnInterruptDisposition::NotStarted
        } else {
            ProviderTurnInterruptDisposition::Started
        };
        let completion = active.completion.subscribe();
        Ok(ProviderTurnControl {
            disposition,
            interruption: active.interruption.clone(),
            completion,
        })
    }
}

fn validate_prepared(
    runtime: &super::OwnedRuntime,
    prepared: &ProviderPreparedTurn,
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
) -> Result<(), ProviderAdapterError> {
    if prepared.room_id != session.public.room_id
        || prepared.session_id != session.public.session_id
        || prepared.execution_id != request.execution_id
        || prepared.turn_id != request.turn_id
        || prepared.turn_generation != request.turn_generation
        || prepared.runtime_handle_id != runtime.handle_id
        || prepared.runtime_owner_id != runtime.owner_id
        || prepared.runtime_lease_token != runtime.lease_token
        || !runtime
            .active_turn
            .as_ref()
            .is_some_and(|active| exact_active(active, prepared))
    {
        return Err(ProviderAdapterError::safe(stale_turn()));
    }
    Ok(())
}

fn exact_active(active: &ActiveProviderTurnSlot, prepared: &ProviderPreparedTurn) -> bool {
    active.preparation_id == prepared.preparation_id
        && active.execution_id == prepared.execution_id
        && active.turn_id == prepared.turn_id
        && active.turn_generation == prepared.turn_generation
}

fn exact_authority_matches(
    active: &ActiveProviderTurnSlot,
    authority: &ProviderExactTurnAuthority,
) -> bool {
    active.execution_id == authority.execution_id
        && active.turn_id == authority.turn_id
        && active.turn_generation == authority.turn_generation
}

fn same_execution(active: &ActiveProviderTurnSlot, request: &ProviderTurnRequest) -> bool {
    active.execution_id == request.execution_id
        && active.turn_id == request.turn_id
        && active.turn_generation == request.turn_generation
}

fn prepared_from_active(
    session: &DurableAgentSession,
    runtime: &super::OwnedRuntime,
    active: &ActiveProviderTurnSlot,
) -> ProviderPreparedTurn {
    ProviderPreparedTurn {
        room_id: session.public.room_id.clone(),
        session_id: session.public.session_id.clone(),
        execution_id: active.execution_id.clone(),
        turn_id: active.turn_id.clone(),
        turn_generation: active.turn_generation,
        runtime_handle_id: runtime.handle_id.clone(),
        runtime_owner_id: runtime.owner_id.clone(),
        runtime_lease_token: runtime.lease_token.clone(),
        preparation_id: active.preparation_id,
    }
}

fn owner_mismatch(session: &DurableAgentSession) -> ProviderAdapterError {
    ProviderAdapterError::uncertain(
        DriverError::new(
            "runtime_owner_mismatch",
            "The provider runtime does not match the durable session authority.",
        ),
        &session.runtime_handle_id,
        &session.runtime_owner_id,
    )
}

const fn stale_turn() -> DriverError {
    DriverError::new(
        "stale_provider_turn",
        "The exact provider turn control no longer owns this runtime generation.",
    )
}

const fn operation_in_progress() -> DriverError {
    DriverError::new(
        "operation_in_progress",
        "Another exact provider operation is already active.",
    )
}
