use std::sync::Arc;

use agentsassemble_domain::DurableAgentSession;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use super::{
    DriverError, LaunchingRuntime, OwnedRuntime, ProviderAdapter, ProviderAdapterError,
    ProviderRuntimeStarted, ProviderStartReservation, RuntimeState, initialize_owned_runtime,
    reuse_owned_runtime, validate_owned_runtime,
};
use crate::{runtime_authority::revalidate_runtime_authority, runtime_lease::HeldRuntimeLease};

impl ProviderAdapter {
    /// Reserves the exact runtime identity and custody lease before any provider launch effect.
    ///
    /// # Errors
    ///
    /// Returns a redacted fail-closed authority or custody error.
    pub async fn reserve_start(
        &self,
        session: &DurableAgentSession,
    ) -> Result<ProviderStartReservation, ProviderAdapterError> {
        let slot = self.slot(session).await;
        let mut slot = slot.lock().await;
        match &slot.state {
            RuntimeState::Launching(runtime) => {
                if runtime.effect_started {
                    return Err(ProviderAdapterError::uncertain_with_lease(
                        DriverError::new(
                            "provider_launch_unconfirmed",
                            "A provider launch is still awaiting a confirmed cleanup.",
                        ),
                        &runtime.handle_id,
                        &runtime.owner_id,
                        runtime.runtime_lease.token(),
                    ));
                }
                if runtime.profile_key != session.runtime_profile_key
                    || (!session.runtime_handle_id.is_empty()
                        && session.runtime_handle_id != runtime.handle_id)
                    || (!session.runtime_owner_id.is_empty()
                        && session.runtime_owner_id != runtime.owner_id)
                    || (!session.runtime_lease_token.is_empty()
                        && session.runtime_lease_token != runtime.runtime_lease.token())
                {
                    return Err(ProviderAdapterError::uncertain(
                        DriverError::new(
                            "runtime_owner_mismatch",
                            "The reserved provider launch does not match durable authority.",
                        ),
                        &runtime.handle_id,
                        &runtime.owner_id,
                    ));
                }
                Ok(ProviderStartReservation {
                    runtime_handle_id: runtime.handle_id.clone(),
                    runtime_owner_id: runtime.owner_id.clone(),
                    runtime_lease_token: runtime.runtime_lease.token().to_owned(),
                })
            }
            RuntimeState::Running(runtime) => {
                validate_owned_runtime(session, runtime)?;
                Ok(ProviderStartReservation {
                    runtime_handle_id: runtime.handle_id.clone(),
                    runtime_owner_id: runtime.owner_id.clone(),
                    runtime_lease_token: runtime.lease_token.clone(),
                })
            }
            RuntimeState::StopConfirmed { .. } => {
                Err(ProviderAdapterError::safe(DriverError::new(
                    "operation_in_progress",
                    "A confirmed provider stop is awaiting its durable checkpoint.",
                )))
            }
            RuntimeState::Vacant => {
                if !session.runtime_handle_id.is_empty()
                    || !session.runtime_owner_id.is_empty()
                    || !session.runtime_lease_token.is_empty()
                {
                    return Err(ProviderAdapterError {
                        code: "runtime_owner_mismatch",
                        message: "The durable provider handle is not owned by this supervisor.",
                        effect_uncertain: true,
                        runtime_handle_id: session.runtime_handle_id.clone(),
                        runtime_owner_id: session.runtime_owner_id.clone(),
                        runtime_lease_token: session.runtime_lease_token.clone(),
                        runtime_stopped: false,
                    });
                }
                revalidate_runtime_authority(session)
                    .await
                    .map_err(ProviderAdapterError::safe)?;
                let runtime_lease =
                    HeldRuntimeLease::prepare(&session.public.room_id, &session.public.session_id)
                        .map_err(|_| {
                            ProviderAdapterError::safe(DriverError::new(
                                "provider_custody_unavailable",
                                "The provider runtime lease could not be established.",
                            ))
                        })?;
                let reservation = ProviderStartReservation {
                    runtime_handle_id: runtime_lease.new_runtime_handle_id(),
                    runtime_owner_id: self.owner.supervisor_id.clone(),
                    runtime_lease_token: runtime_lease.token().to_owned(),
                };
                slot.state = RuntimeState::Launching(LaunchingRuntime {
                    handle_id: reservation.runtime_handle_id.clone(),
                    owner_id: reservation.runtime_owner_id.clone(),
                    profile_key: session.runtime_profile_key.clone(),
                    effect_started: false,
                    runtime_lease,
                });
                Ok(reservation)
            }
        }
    }

    /// Removes an exact pre-effect launch reservation after its durable authorization failed.
    pub async fn cancel_start_reservation(
        &self,
        room_id: &str,
        session_id: &str,
        reservation: &ProviderStartReservation,
    ) {
        let Some(slot) = self.existing_slot(room_id, session_id).await else {
            return;
        };
        let mut slot = slot.lock().await;
        if !matches!(
            &slot.state,
            RuntimeState::Launching(runtime)
                if !runtime.effect_started
                    && runtime.handle_id == reservation.runtime_handle_id
                    && runtime.owner_id == reservation.runtime_owner_id
                    && runtime.runtime_lease.token() == reservation.runtime_lease_token
        ) {
            return;
        }
        let RuntimeState::Launching(runtime) =
            std::mem::replace(&mut slot.state, RuntimeState::Vacant)
        else {
            unreachable!("matched launch reservation must remain owned");
        };
        runtime.runtime_lease.cleanup_pre_effect();
    }

    /// Releases exact proven-absent start authority only after its DB transition commits.
    pub async fn release_checkpointed_start_absence(&self, session: &DurableAgentSession) {
        let Some(slot) = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
        else {
            return;
        };
        let mut slot = slot.lock().await;
        let exact_launching = matches!(
            &slot.state,
            RuntimeState::Launching(runtime)
                if runtime.handle_id == session.runtime_handle_id
                    && runtime.owner_id == session.runtime_owner_id
                    && runtime.runtime_lease.token() == session.runtime_lease_token
        );
        let exact_stopped = matches!(
            &slot.state,
            RuntimeState::StopConfirmed {
                handle_id,
                owner_id,
                runtime_lease,
            } if handle_id == &session.runtime_handle_id
                && owner_id == &session.runtime_owner_id
                && runtime_lease.token() == session.runtime_lease_token
        );
        if !exact_launching && !exact_stopped {
            return;
        }
        match std::mem::replace(&mut slot.state, RuntimeState::Vacant) {
            RuntimeState::Launching(mut runtime) => runtime.runtime_lease.release_and_remove(),
            RuntimeState::StopConfirmed {
                mut runtime_lease, ..
            } => runtime_lease.release_and_remove(),
            _ => unreachable!("matched failed start must retain exact runtime authority"),
        }
    }

    /// Starts or proves one exact durably authorized session runtime.
    ///
    /// # Errors
    ///
    /// Returns a redacted fail-closed runtime error.
    pub async fn start_reserved(
        &self,
        session: &DurableAgentSession,
    ) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
        let slot = self.slot(session).await;
        let mut slot = slot.lock().await;
        match &mut slot.state {
            RuntimeState::Launching(runtime)
                if runtime.handle_id != session.runtime_handle_id
                    || runtime.owner_id != session.runtime_owner_id
                    || runtime.runtime_lease.token() != session.runtime_lease_token
                    || runtime.profile_key != session.runtime_profile_key =>
            {
                Err(ProviderAdapterError::uncertain(
                    DriverError::new(
                        "runtime_owner_mismatch",
                        "The durable provider launch authority does not match its reservation.",
                    ),
                    &runtime.handle_id,
                    &runtime.owner_id,
                ))
            }
            RuntimeState::Launching(_) => {
                super::launch_state::begin_launch_effect(&mut slot)?;
                let RuntimeState::Launching(runtime) = &mut slot.state else {
                    unreachable!("authorized provider runtime must remain launching");
                };
                runtime.effect_started = true;
                let launch = {
                    let RuntimeState::Launching(runtime) = &slot.state else {
                        unreachable!("authorized provider runtime must be launching");
                    };
                    self.owner
                        .factory
                        .launch(session, &runtime.runtime_lease)
                        .await
                };
                let driver = match launch {
                    Ok(driver) => driver,
                    Err(failure) if failure.effect_uncertain => {
                        let RuntimeState::Launching(runtime) = &slot.state else {
                            unreachable!("uncertain provider launch must remain owned");
                        };
                        return Err(ProviderAdapterError::uncertain(
                            failure.error,
                            &runtime.handle_id,
                            &runtime.owner_id,
                        ));
                    }
                    Err(failure) => {
                        let RuntimeState::Launching(runtime) =
                            std::mem::replace(&mut slot.state, RuntimeState::Vacant)
                        else {
                            unreachable!("safe provider launch failure must remain owned");
                        };
                        let error = ProviderAdapterError::confirmed_stopped(
                            failure.error,
                            &runtime.handle_id,
                            &runtime.owner_id,
                            runtime.runtime_lease.token(),
                        );
                        slot.state = RuntimeState::StopConfirmed {
                            handle_id: runtime.handle_id,
                            owner_id: runtime.owner_id,
                            runtime_lease: runtime.runtime_lease,
                        };
                        return Err(error);
                    }
                };
                let RuntimeState::Launching(runtime) =
                    std::mem::replace(&mut slot.state, RuntimeState::Vacant)
                else {
                    unreachable!("completed provider launch must remain owned");
                };
                slot.state = RuntimeState::Running(OwnedRuntime {
                    handle_id: runtime.handle_id,
                    owner_id: runtime.owner_id,
                    lease_token: runtime.runtime_lease.token().to_owned(),
                    profile_key: session.runtime_profile_key.clone(),
                    driver: Arc::new(Mutex::new(driver)),
                    turn_cancellation: CancellationToken::new(),
                    runtime_lease: Some(runtime.runtime_lease),
                });
                let RuntimeState::Running(runtime) = &mut slot.state else {
                    unreachable!("new provider runtime slot must be running");
                };
                initialize_owned_runtime(session, runtime).await
            }
            RuntimeState::Running(runtime) => reuse_owned_runtime(session, runtime).await,
            RuntimeState::StopConfirmed { .. } => {
                Err(ProviderAdapterError::safe(DriverError::new(
                    "operation_in_progress",
                    "A confirmed provider stop is awaiting its durable checkpoint.",
                )))
            }
            RuntimeState::Vacant => Err(ProviderAdapterError::safe(DriverError::new(
                "runtime_start_not_authorized",
                "The provider launch has no durable pre-effect authorization.",
            ))),
        }
    }

    /// Starts a runtime for provider-unit tests without product persistence.
    ///
    /// # Errors
    ///
    /// Returns a redacted fail-closed runtime error.
    #[cfg(test)]
    pub(crate) async fn start(
        &self,
        session: &DurableAgentSession,
    ) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
        let reservation = self.reserve_start(session).await?;
        let mut authorized = session.clone();
        authorized.runtime_handle_id = reservation.runtime_handle_id;
        authorized.runtime_owner_id = reservation.runtime_owner_id;
        authorized.runtime_lease_token = reservation.runtime_lease_token;
        self.start_reserved(&authorized).await
    }
}
