use agentsassemble_domain::DurableAgentSession;

use super::{
    DriverError, ProviderAdapter, ProviderAdapterError, ProviderResidentRuntime, RuntimeState,
    observation::running_health_reason,
};

impl ProviderAdapter {
    /// Proves that an idle durable session still names this exact live runtime.
    ///
    /// This command-triggered check consults only the adapter's live slot and driver. It does not
    /// poll, retry, or revalidate filesystem selection authority for an already-running process.
    ///
    /// # Errors
    ///
    /// Returns a closed failure when the slot, identity, or driver health is not exact.
    pub async fn prove_resident_runtime(
        &self,
        session: &DurableAgentSession,
    ) -> Result<ProviderResidentRuntime, ProviderAdapterError> {
        let slot = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
            .ok_or_else(resident_runtime_unavailable)?;
        let mut slot = slot.lock().await;
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return Err(resident_runtime_unavailable());
        };
        if runtime.handle_id != session.runtime_handle_id
            || runtime.owner_id != session.runtime_owner_id
            || runtime.lease_token != session.runtime_lease_token
            || runtime.profile_key != session.runtime_profile_key
        {
            return Err(resident_runtime_mismatch());
        }
        if runtime.active_turn.is_some() {
            return Err(ProviderAdapterError::safe(DriverError::new(
                "provider_turn_active",
                "The resident provider runtime still owns an active turn.",
            )));
        }
        if let Some(reason_code) = running_health_reason(runtime).await {
            return Err(ProviderAdapterError::safe(DriverError::new(
                reason_code,
                "The resident provider runtime is not available for a scheduling-state change.",
            )));
        }
        Ok(ProviderResidentRuntime {
            runtime_handle_id: runtime.handle_id.clone(),
            runtime_owner_id: runtime.owner_id.clone(),
            runtime_lease_token: runtime.lease_token.clone(),
            runtime_profile_key: runtime.profile_key.clone(),
        })
    }
}

fn resident_runtime_unavailable() -> ProviderAdapterError {
    ProviderAdapterError::safe(DriverError::new(
        "resident_runtime_unavailable",
        "The resident provider runtime is not owned by this server process.",
    ))
}

fn resident_runtime_mismatch() -> ProviderAdapterError {
    ProviderAdapterError::safe(DriverError::new(
        "resident_runtime_mismatch",
        "The resident provider runtime identity no longer matches durable state.",
    ))
}
