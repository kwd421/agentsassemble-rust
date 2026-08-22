use agentsassemble_domain::{DurableAgentSession, has_visible_text};

use super::{
    DriverError, ProviderAdapter, ProviderAdapterError, RuntimeState, revalidate_runtime_authority,
    validate_owned_runtime,
};

const MAX_TURN_ID_BYTES: usize = 128;
const MAX_PROVIDER_INPUT_CHARS: usize = 20_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnRequest {
    pub turn_id: String,
    pub input: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnCompleted {
    pub turn_id: String,
    pub provider_turn_id: String,
    pub content: String,
}

impl ProviderAdapter {
    /// Runs one durable assigned turn through the exact owned provider session.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed authority, process, protocol, or provider-turn error.
    pub async fn send_turn(
        &self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnCompleted, ProviderAdapterError> {
        validate_request(session, request).map_err(ProviderAdapterError::safe)?;
        let slot = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
            .ok_or_else(|| owner_mismatch(session))?;
        let (driver, cancellation, handle_id, owner_id) = {
            let slot = slot.lock().await;
            let RuntimeState::Running(runtime) = &slot.state else {
                return Err(owner_mismatch(session));
            };
            validate_owned_runtime(session, runtime)?;
            validate_exact_owner(session, runtime)?;
            revalidate_runtime_authority(session)
                .await
                .map_err(|error| {
                    ProviderAdapterError::uncertain(error, &runtime.handle_id, &runtime.owner_id)
                })?;
            (
                runtime.driver.clone(),
                runtime.turn_cancellation.clone(),
                runtime.handle_id.clone(),
                runtime.owner_id.clone(),
            )
        };
        let mut driver = driver.lock().await;
        if cancellation.is_cancelled() {
            return Err(ProviderAdapterError::uncertain(
                turn_cancelled(),
                &handle_id,
                &owner_id,
            ));
        }
        match driver.is_alive() {
            Ok(true) => {}
            Ok(false) => {
                return Err(ProviderAdapterError::uncertain(
                    DriverError::new(
                        "provider_runtime_exited",
                        "The owned provider runtime exited before its assigned turn.",
                    ),
                    &handle_id,
                    &owner_id,
                ));
            }
            Err(error) => {
                return Err(ProviderAdapterError::uncertain(
                    error, &handle_id, &owner_id,
                ));
            }
        }
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(turn_cancelled()),
            result = driver.send_turn(session, request) => result,
        };
        result.map_err(|error| ProviderAdapterError::uncertain(error, &handle_id, &owner_id))
    }
}

fn validate_request(
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
) -> Result<(), DriverError> {
    if request.turn_id.is_empty()
        || request.turn_id.len() > MAX_TURN_ID_BYTES
        || request.turn_id.trim() != request.turn_id
        || request.turn_id.chars().any(char::is_control)
        || request.turn_id != session.public.active_turn_id
    {
        return Err(DriverError::new(
            "provider_turn_conflict",
            "The provider turn does not match durable assignment authority.",
        ));
    }
    if !matches!(session.public.turn_phase.as_str(), "thinking" | "streaming") {
        return Err(DriverError::new(
            "provider_turn_phase_invalid",
            "The durable provider turn is not in an active phase.",
        ));
    }
    if request.input.chars().count() > MAX_PROVIDER_INPUT_CHARS
        || request.input.contains('\0')
        || !has_visible_text(&request.input)
    {
        return Err(DriverError::new(
            "provider_turn_input_invalid",
            "The provider turn input is empty or exceeds its bound.",
        ));
    }
    Ok(())
}

fn validate_exact_owner(
    session: &DurableAgentSession,
    runtime: &super::OwnedRuntime,
) -> Result<(), ProviderAdapterError> {
    if session.runtime_handle_id != runtime.handle_id
        || session.runtime_owner_id != runtime.owner_id
        || session.provider_session_id.is_empty()
        || !session.public.provider_session_active
        || !session.public.enabled
        || session.public.status != "attached"
        || !matches!(session.public.runtime_status.as_str(), "idle" | "busy")
    {
        return Err(owner_mismatch(session));
    }
    Ok(())
}

fn owner_mismatch(session: &DurableAgentSession) -> ProviderAdapterError {
    ProviderAdapterError::uncertain(
        DriverError::new(
            "runtime_owner_mismatch",
            "The durable provider turn does not match the owned runtime.",
        ),
        &session.runtime_handle_id,
        &session.runtime_owner_id,
    )
}

const fn turn_cancelled() -> DriverError {
    DriverError::new(
        "provider_turn_cancelled",
        "The provider turn was cancelled for owned runtime shutdown.",
    )
}
