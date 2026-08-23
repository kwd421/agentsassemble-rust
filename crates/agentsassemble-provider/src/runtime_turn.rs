use std::collections::HashSet;

use agentsassemble_domain::{DurableAgentSession, has_visible_text};

use super::{
    DriverError, ProviderAdapter, ProviderAdapterError, ProviderDriver, RuntimeState,
    revalidate_runtime_authority, validate_owned_runtime,
};
use crate::room_portal::ProviderTurnOutcome;

const MAX_TURN_ID_BYTES: usize = 128;
const MAX_PROVIDER_INPUT_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
const MAX_ROOM_AGENT_IDS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnRequest {
    pub turn_id: String,
    pub input: String,
    pub room_observation: Option<ProviderRoomObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRoomObservation {
    pub input_up_to_seq: i64,
    pub view: String,
    pub allowed_agent_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnCompleted {
    pub turn_id: String,
    pub provider_turn_id: String,
    pub provider_session_id: Option<String>,
    pub outcome: ProviderTurnOutcome,
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
        match driver.is_alive().await {
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
        if request.room_observation.is_some() {
            driver
                .begin_room_observation(request)
                .map_err(ProviderAdapterError::safe)?;
        }
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(turn_cancelled()),
            result = driver.send_turn(session, request) => result,
        };
        match result {
            Ok(completed) => finish_completed_turn(
                driver.as_mut(),
                session,
                request,
                completed,
                &handle_id,
                &owner_id,
            ),
            Err(error) => {
                driver.abort_room_observation();
                let requires_restart = driver.requires_restart();
                drop(driver);
                if requires_restart {
                    return match self
                        .stop(
                            &session.public.room_id,
                            &session.public.session_id,
                            &handle_id,
                            &owner_id,
                        )
                        .await
                    {
                        Ok(()) => Err(ProviderAdapterError::confirmed_stopped(
                            error, &handle_id, &owner_id,
                        )),
                        Err(stop_error) => Err(stop_error),
                    };
                }
                Err(ProviderAdapterError::uncertain(
                    error, &handle_id, &owner_id,
                ))
            }
        }
    }
}

fn finish_completed_turn(
    driver: &mut dyn ProviderDriver,
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    mut completed: ProviderTurnCompleted,
    handle_id: &str,
    owner_id: &str,
) -> Result<ProviderTurnCompleted, ProviderAdapterError> {
    if completed.turn_id != request.turn_id
        || completed.provider_turn_id.is_empty()
        || completed.provider_turn_id.len() > MAX_TURN_ID_BYTES
        || completed.provider_turn_id.trim() != completed.provider_turn_id
        || completed.provider_turn_id.chars().any(char::is_control)
        || !valid_provider_session_transition(session, completed.provider_session_id.as_deref())
    {
        driver.abort_room_observation();
        return Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "provider_protocol_invalid",
                "The provider returned invalid turn or session authority.",
            ),
            handle_id,
            owner_id,
        ));
    }
    if request.room_observation.is_some() {
        completed.outcome = match driver.finish_room_observation(request) {
            Ok(outcome) => outcome,
            Err(error) => {
                driver.abort_room_observation();
                return Err(ProviderAdapterError::uncertain(error, handle_id, owner_id));
            }
        };
    }
    Ok(completed)
}

fn valid_provider_session_transition(
    session: &DurableAgentSession,
    provider_session_id: Option<&str>,
) -> bool {
    let Some(next) = provider_session_id else {
        return true;
    };
    if next.is_empty()
        || next.len() > 200
        || next.trim() != next
        || next.chars().any(char::is_control)
        || next.starts_with("pending-antigravity-")
    {
        return false;
    }
    next == session.provider_session_id
        || (session.public.provider_kind == "antigravity_live_session"
            && session
                .provider_session_id
                .starts_with("pending-antigravity-"))
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
    if let Some(observation) = &request.room_observation {
        let unique_ids = observation.allowed_agent_ids.iter().collect::<HashSet<_>>();
        if observation.input_up_to_seq <= 0
            || observation.view.chars().count() > MAX_ROOM_VIEW_CHARS
            || observation.view.len() > MAX_ROOM_VIEW_BYTES
            || observation.view.contains('\0')
            || !has_visible_text(&observation.view)
            || observation.allowed_agent_ids.len() > MAX_ROOM_AGENT_IDS
            || unique_ids.len() != observation.allowed_agent_ids.len()
            || observation.allowed_agent_ids.iter().any(|agent_id| {
                agent_id.is_empty()
                    || agent_id.len() > MAX_TURN_ID_BYTES
                    || agent_id.trim() != agent_id
                    || agent_id.chars().any(char::is_control)
            })
        {
            return Err(DriverError::new(
                "room_observation_invalid",
                "The provider room observation is invalid or exceeds its bound.",
            ));
        }
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
