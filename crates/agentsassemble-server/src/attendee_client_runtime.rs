//! Client-owned provider lifetime. This owner never reads or writes a room database.
use agentsassemble_domain::{
    AgentRuntimeCustody, AgentRuntimeStatus, AgentSessionDraft, AgentSessionStatus,
    DurableAgentSession,
};
use agentsassemble_persistence::{AttendeeCleanupDelivery, AttendeeRuntimeReady};
use agentsassemble_provider::ProviderAdapter;

use crate::{AttendeeClientError, AttendeeJoined};

#[path = "attendee_client_execution.rs"]
mod execution;
#[path = "attendee_client_interrupt.rs"]
mod interrupt;
pub use execution::AttendeeExecution;
pub use interrupt::AttendeeInterrupt;

pub struct AttendeeRuntime {
    pub(crate) session: DurableAgentSession,
    pub(crate) adapter: ProviderAdapter,
    ready: Option<AttendeeRuntimeReady>,
    stopped: bool,
    pub(super) persona: Option<agentsassemble_domain::PersonaCard>,
}

impl AttendeeRuntime {
    /// Takes exclusive ownership of a fresh adapter and a locally validated provider selection.
    ///
    /// # Errors
    /// Rejects provider mismatch and a persona that differs from the locally resolved selection.
    pub fn new(
        joined: &AttendeeJoined,
        mut draft: AgentSessionDraft,
        adapter: ProviderAdapter,
        persona: Option<agentsassemble_domain::PersonaCard>,
    ) -> Result<Self, AttendeeClientError> {
        if draft.provider_kind != joined.provider_kind
            || persona.as_ref().map_or("", |card| card.id.as_str()) != draft.persona_card_id
        {
            return Err(AttendeeClientError::local("attendee_profile_mismatch"));
        }
        draft.agent_id.clone_from(&joined.participant_id);
        let session = draft.initial_session(
            &joined.room_id,
            AgentRuntimeCustody::External,
            chrono::Utc::now(),
        );
        Ok(Self {
            session,
            adapter,
            ready: None,
            stopped: false,
            persona,
        })
    }

    /// Reserves actual local custody before starting the provider through its canonical adapter.
    ///
    /// # Errors
    /// Returns redacted launch errors. The owner must still be stopped after a failed launch.
    pub async fn start(
        &mut self,
        cancellation: Option<&tokio_util::sync::CancellationToken>,
    ) -> Result<AttendeeRuntimeReady, AttendeeClientError> {
        if self.stopped {
            return Err(AttendeeClientError::local("attendee_runtime_stopped"));
        }
        if let Some(ready) = &self.ready {
            return Ok(ready.clone());
        }
        let reserved = self
            .adapter
            .reserve_start(&self.session)
            .await
            .map_err(|error| provider_error(&error))?;
        self.session.runtime_handle_id = reserved.runtime_handle_id;
        self.session.runtime_owner_id = reserved.runtime_owner_id;
        self.session.runtime_lease_token = reserved.runtime_lease_token;
        let started = self
            .adapter
            .start_reserved(&self.session, cancellation)
            .await
            .map_err(|error| provider_error(&error))?;
        self.session.provider_session_id = started.provider_session_id;
        self.session.public.provider_session_active = started.provider_session_active;
        self.session.public.provider_session_reused = started.provider_session_reused;
        self.session.public.runtime_status = AgentRuntimeStatus::Idle;
        self.session.public.status = AgentSessionStatus::Attached;
        self.session.public.enabled = true;
        let profile = &self.session.public;
        let ready = AttendeeRuntimeReady {
            retained_interrupt: started.turn_interrupt
                == agentsassemble_domain::ProviderTurnInterrupt::RetainedRuntime,
            runtime_handle_id: self.session.runtime_handle_id.clone(),
            runtime_owner_id: self.session.runtime_owner_id.clone(),
            runtime_lease_token: self.session.runtime_lease_token.clone(),
            provider_session_id: self.session.provider_session_id.clone(),
            model: profile.model.clone(),
            reasoning_effort: profile.reasoning_effort.clone(),
            service_tier: profile.service_tier.clone(),
            variant: profile.variant.clone(),
            execution_harness: profile.execution_harness.clone(),
            permission_mode: profile.permission_mode.clone(),
            max_output_tokens: profile.max_output_tokens,
        };
        self.ready = Some(ready.clone());
        Ok(ready)
    }

    /// Stops this client's adapter and requires positive observation of any reserved runtime.
    ///
    /// # Errors
    /// Unconfirmed launch or process cleanup remains an error, including after network loss.
    pub async fn stop(&mut self) -> Result<(), AttendeeClientError> {
        if self.stopped {
            return Ok(());
        }
        let outcome = self.adapter.shutdown_with_observations().await;
        if let Some(error) = outcome.failure {
            return Err(provider_error(&error));
        }
        if !self.session.runtime_handle_id.is_empty()
            && !outcome.gone.iter().any(|gone| {
                gone.room_id == self.session.public.room_id
                    && gone.session_id == self.session.public.session_id
                    && gone.runtime_handle_id == self.session.runtime_handle_id
                    && gone.runtime_owner_id == self.session.runtime_owner_id
                    && gone.runtime_lease_token == self.session.runtime_lease_token
            })
        {
            return Err(AttendeeClientError::local(
                "attendee_runtime_cleanup_unconfirmed",
            ));
        }
        self.stopped = true;
        self.session.public.provider_session_active = false;
        self.session.public.runtime_status = AgentRuntimeStatus::Stopped;
        Ok(())
    }

    /// Releases local leases after a cleanup receipt, or confirmed leave with no stop delivery.
    /// The caller must finish the remote protocol before acknowledging local cleanup.
    ///
    /// # Errors
    /// Rejects cleanup that has not been positively observed for this runtime.
    pub async fn acknowledge_cleanup(
        &self,
        delivery: Option<&AttendeeCleanupDelivery>,
    ) -> Result<(), AttendeeClientError> {
        if let Some(delivery) = delivery {
            self.verify_cleanup(delivery)?;
        } else if !self.stopped {
            return Err(AttendeeClientError::local(
                "attendee_runtime_cleanup_unconfirmed",
            ));
        }
        self.adapter
            .release_confirmed_stop(
                &self.session.public.room_id,
                &self.session.public.session_id,
                &self.session.runtime_handle_id,
                &self.session.runtime_owner_id,
                &self.session.runtime_lease_token,
            )
            .await;
        Ok(())
    }

    fn owns_runtime(
        &self,
        authority: &agentsassemble_persistence::ProviderTurnStartAuthority,
    ) -> bool {
        authority.room_id == self.session.public.room_id
            && authority.session_id == self.session.public.session_id
            && authority.runtime_handle_id == self.session.runtime_handle_id
            && authority.runtime_owner_id == self.session.runtime_owner_id
            && authority.runtime_lease_token == self.session.runtime_lease_token
    }

    /// Verifies server-requested custody against the positively stopped local owner.
    /// An empty server tuple means readiness was never committed, not that local launch was absent.
    ///
    /// # Errors
    /// Rejects unconfirmed local stop or another runtime's cleanup request.
    pub fn verify_cleanup(
        &self,
        delivery: &AttendeeCleanupDelivery,
    ) -> Result<(), AttendeeClientError> {
        let unbound = delivery.runtime_handle_id.is_empty()
            && delivery.runtime_owner_id.is_empty()
            && delivery.runtime_lease_token.is_empty();
        let exact = delivery.runtime_handle_id == self.session.runtime_handle_id
            && delivery.runtime_owner_id == self.session.runtime_owner_id
            && delivery.runtime_lease_token == self.session.runtime_lease_token;
        if !self.stopped || delivery.cleanup_id.is_nil() || (!unbound && !exact) {
            return Err(AttendeeClientError::local(
                "attendee_cleanup_authority_mismatch",
            ));
        }
        Ok(())
    }
}

pub(crate) fn provider_error(
    error: &agentsassemble_provider::ProviderAdapterError,
) -> AttendeeClientError {
    AttendeeClientError::local(&error.code)
}
