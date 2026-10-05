//! Exact client-owned interruption and positive quiescence receipts survive reconnect.
use super::{AttendeeClientError, AttendeeExecution, AttendeeRuntime, provider_error};
use agentsassemble_domain::{AgentRuntimeStatus, AgentTurnPhase};
use agentsassemble_persistence::{
    AttendeeInterruptDelivery, AttendeeInterruptReport, AttendeeInterruptedRuntime,
};
use agentsassemble_provider::{
    ProviderExactTurnAuthority, ProviderRuntimeObservation, ProviderTurnQuiescence,
};
use tokio::task::JoinHandle;
use uuid::Uuid;

pub struct AttendeeInterrupt {
    delivery: AttendeeInterruptDelivery,
    task: Option<JoinHandle<Result<AttendeeInterruptedRuntime, AttendeeClientError>>>,
    report: Option<AttendeeInterruptReport>,
    request_id: Uuid,
}

impl AttendeeRuntime {
    /// Applies one server-owned interrupt to the matching local execution or never-entered turn.
    ///
    /// # Errors
    /// Rejects another runtime/execution and any missing local evidence of pre-entry custody.
    pub fn interrupt(
        &mut self,
        delivery: AttendeeInterruptDelivery,
        execution: Option<&AttendeeExecution>,
    ) -> Result<AttendeeInterrupt, AttendeeClientError> {
        if self.stopped || self.ready.is_none() || !self.owns_runtime(&delivery.authority) {
            return Err(mismatch());
        }
        if let Some(execution) = execution {
            if !execution.matches_authority(&delivery.authority)
                || self.session.turn_generation != delivery.authority.turn_generation
                || self.session.public.active_turn_id != delivery.authority.turn_id
            {
                return Err(mismatch());
            }
        } else {
            // This exclusive live client owner has never entered the newer generation. No
            // provider input, artificial turn slot or claimed server Assigned phase is created.
            if !self.session.public.active_turn_id.is_empty()
                || delivery.authority.turn_generation <= self.session.turn_generation
            {
                return Err(mismatch());
            }
            self.session.turn_generation = delivery.authority.turn_generation;
        }
        let adapter = self.adapter.clone();
        let session = self.session.clone();
        let authority = exact(&delivery);
        let entered = execution.is_some();
        let task = tokio::spawn(async move {
            if entered {
                let mut control = adapter
                    .begin_exact_turn(&authority)
                    .await
                    .map_err(|error| provider_error(&error))?;
                control.request_interrupt();
                match control
                    .wait_quiesced(crate::provider_turn_interrupt_runtime::QUIESCENCE_TIMEOUT)
                    .await
                    .map_err(|error| provider_error(&error))?
                {
                    ProviderTurnQuiescence::RuntimeRetained => {
                        Ok(AttendeeInterruptedRuntime::Retained)
                    }
                    ProviderTurnQuiescence::RuntimeGone => Ok(AttendeeInterruptedRuntime::Gone),
                }
            } else {
                match adapter.observe(&session).await {
                    ProviderRuntimeObservation::Adopted {
                        handle_id,
                        new_owner_id,
                        runtime_profile_key,
                        ..
                    } if handle_id == session.runtime_handle_id
                        && new_owner_id == session.runtime_owner_id
                        && runtime_profile_key == session.runtime_profile_key =>
                    {
                        Ok(AttendeeInterruptedRuntime::Retained)
                    }
                    ProviderRuntimeObservation::Gone => Ok(AttendeeInterruptedRuntime::Gone),
                    _ => Err(AttendeeClientError::local(
                        "attendee_runtime_observation_unconfirmed",
                    )),
                }
            }
        });
        Ok(AttendeeInterrupt {
            delivery,
            task: Some(task),
            report: None,
            request_id: Uuid::new_v4(),
        })
    }
}

impl AttendeeInterrupt {
    #[must_use]
    pub fn matches_delivery(&self, delivery: &AttendeeInterruptDelivery) -> bool {
        &self.delivery == delivery
    }

    #[must_use]
    pub fn is_running(&self) -> bool {
        self.task.is_some()
    }

    /// Waits on the owned interruption task and fixes one report identity after positive proof.
    ///
    /// # Errors
    /// Preserves uncertain interruption, native cleanup, and lost-task failures.
    pub async fn complete(&mut self) -> Result<(), AttendeeClientError> {
        let Some(task) = &mut self.task else {
            return Ok(());
        };
        let completed = task.await;
        self.task = None;
        let runtime = completed
            .map_err(|_| AttendeeClientError::local("attendee_interrupt_owner_unresolved"))??;
        self.report = Some(AttendeeInterruptReport {
            request_id: self.request_id,
            interrupted: self.delivery.clone(),
            runtime,
        });
        Ok(())
    }

    #[must_use]
    pub fn report(&self) -> Option<&AttendeeInterruptReport> {
        self.report.as_ref()
    }

    /// Finalizes local custody after the caller receives this exact committed interrupt receipt.
    ///
    /// # Errors
    /// Rejects missing proof, changed local custody, and failure to join the quiesced task.
    pub async fn acknowledge(
        &self,
        runtime: &mut AttendeeRuntime,
        execution: Option<&mut AttendeeExecution>,
    ) -> Result<(), AttendeeClientError> {
        let report = self.report.as_ref().ok_or_else(mismatch)?;
        if !runtime.owns_runtime(&self.delivery.authority)
            || runtime.session.turn_generation != self.delivery.authority.turn_generation
        {
            return Err(mismatch());
        }
        if let Some(execution) = execution {
            if !execution.matches_authority(&self.delivery.authority) {
                return Err(mismatch());
            }
            execution.drain_after_quiescence().await?;
        }
        match report.runtime {
            AttendeeInterruptedRuntime::Retained => {
                runtime
                    .adapter
                    .release_terminal_turn(&exact(&self.delivery))
                    .await;
                runtime.session.public.runtime_status = AgentRuntimeStatus::Idle;
            }
            AttendeeInterruptedRuntime::Gone => {
                runtime.stopped = true;
                runtime.session.public.provider_session_active = false;
                runtime.session.public.runtime_status = AgentRuntimeStatus::Stopped;
                runtime
                    .adapter
                    .release_confirmed_stop(
                        &runtime.session.public.room_id,
                        &runtime.session.public.session_id,
                        &runtime.session.runtime_handle_id,
                        &runtime.session.runtime_owner_id,
                        &runtime.session.runtime_lease_token,
                    )
                    .await;
            }
        }
        runtime.session.public.active_turn_id.clear();
        runtime.session.public.turn_phase = AgentTurnPhase::None;
        Ok(())
    }
}

fn exact(delivery: &AttendeeInterruptDelivery) -> ProviderExactTurnAuthority {
    let start = &delivery.authority;
    ProviderExactTurnAuthority {
        room_id: start.room_id.clone(),
        session_id: start.session_id.clone(),
        execution_id: start.execution_id.clone(),
        turn_id: start.turn_id.clone(),
        turn_generation: start.turn_generation,
        runtime_handle_id: start.runtime_handle_id.clone(),
        runtime_owner_id: start.runtime_owner_id.clone(),
        runtime_lease_token: start.runtime_lease_token.clone(),
    }
}
fn mismatch() -> AttendeeClientError {
    AttendeeClientError::local("attendee_interrupt_authority_mismatch")
}
