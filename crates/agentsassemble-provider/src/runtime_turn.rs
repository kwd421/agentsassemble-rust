use std::{collections::HashSet, panic::AssertUnwindSafe};

use agentsassemble_domain::{DurableAgentSession, has_visible_text};
use futures_util::FutureExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::room_attachment::valid_observation_attachments;

#[cfg(test)]
use super::ProviderExactTurnAuthority;
use super::{
    DriverError, ProviderAdapter, ProviderAdapterError, ProviderDriver, ProviderPreparedTurn,
    RuntimeState, revalidate_runtime_authority, validate_owned_runtime,
};
use crate::driver::{ProviderTurnCompleted, ProviderTurnRequest};

const MAX_TURN_ID_BYTES: usize = 128;
const MAX_PROVIDER_INPUT_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_CHARS: usize = 20_000;
const MAX_ROOM_VIEW_BYTES: usize = 96 * 1024;
const MAX_ROOM_AGENT_IDS: usize = 64;

impl ProviderAdapter {
    /// Runs one durable assigned turn through the exact owned provider session.
    ///
    /// # Errors
    ///
    /// Returns a fail-closed authority, process, protocol, or provider-turn error.
    pub async fn send_prepared_turn(
        &self,
        prepared: ProviderPreparedTurn,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnCompleted, ProviderAdapterError> {
        validate_request(session, request).map_err(ProviderAdapterError::safe)?;
        let owner_authority = match resolve_turn_owner_authority(self, session).await {
            Ok(authority) => authority,
            Err(error) => {
                let result = Err(error);
                self.retain_prepared_turn_result(&prepared, &result, false)
                    .await;
                return result;
            }
        };
        let exact_interruption = self
            .enter_prepared_turn(&prepared, session, request)
            .await?;
        let handle_id = owner_authority.handle_id.clone();
        let owner_id = owner_authority.owner_id.clone();
        let owner_task = tokio::spawn(run_owned_turn_task(
            self.clone(),
            prepared,
            session.clone(),
            request.clone(),
            exact_interruption,
            owner_authority,
        ));
        let turn_outcome = owner_task
            .await
            .unwrap_or_else(|_| provider_turn_owner_failed(&handle_id, &owner_id));
        turn_outcome.result
    }

    /// Runs a provider turn without product persistence only in provider-unit tests.
    #[cfg(test)]
    pub(crate) async fn send_turn(
        &self,
        session: &DurableAgentSession,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnCompleted, ProviderAdapterError> {
        let prepared = match self.prepare_turn(session, request).await {
            Ok(prepared) => prepared,
            Err(error) if error.code == "operation_in_progress" => {
                let authority = ProviderExactTurnAuthority {
                    room_id: session.public.room_id.clone(),
                    session_id: session.public.session_id.clone(),
                    execution_id: request.execution_id.clone(),
                    turn_id: request.turn_id.clone(),
                    turn_generation: request.turn_generation,
                    runtime_handle_id: session.runtime_handle_id.clone(),
                    runtime_owner_id: session.runtime_owner_id.clone(),
                    runtime_lease_token: session.runtime_lease_token.clone(),
                };
                let Some(result) = self.retained_turn_result(&authority).await else {
                    return Err(error);
                };
                self.release_terminal_turn(&authority).await;
                return result;
            }
            Err(error) => return Err(error),
        };
        let authority = prepared.exact_authority();
        let result = self.send_prepared_turn(prepared, session, request).await;
        self.release_terminal_turn(&authority).await;
        result
    }
}

struct TurnRuntimeAuthority {
    driver_cell: std::sync::Arc<super::runtime_driver::DriverCell>,
    cancellation: CancellationToken,
    handle_id: String,
    owner_id: String,
    lease_token: String,
}

async fn resolve_turn_owner_authority(
    adapter: &ProviderAdapter,
    session: &DurableAgentSession,
) -> Result<TurnRuntimeAuthority, ProviderAdapterError> {
    let slot = adapter
        .existing_slot(&session.public.room_id, &session.public.session_id)
        .await
        .ok_or_else(|| owner_mismatch(session))?;
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
    Ok(TurnRuntimeAuthority {
        driver_cell: runtime.driver.clone(),
        cancellation: runtime.turn_cancellation.clone(),
        handle_id: runtime.handle_id.clone(),
        owner_id: runtime.owner_id.clone(),
        lease_token: runtime.lease_token.clone(),
    })
}

async fn run_owned_turn_task(
    adapter: ProviderAdapter,
    prepared: ProviderPreparedTurn,
    session: DurableAgentSession,
    request: ProviderTurnRequest,
    exact_interruption: CancellationToken,
    authority: TurnRuntimeAuthority,
) -> OwnedTurnResult {
    let mut outcome = AssertUnwindSafe(run_turn_owner(TurnOwnerInput {
        driver_cell: authority.driver_cell,
        cancellation: authority.cancellation,
        session: session.clone(),
        request,
        handle_id: authority.handle_id.clone(),
        owner_id: authority.owner_id.clone(),
        exact_interruption,
    }))
    .catch_unwind()
    .await
    .unwrap_or_else(|_| provider_turn_owner_failed(&authority.handle_id, &authority.owner_id));
    if let Err(error) = &outcome.result
        && outcome.requires_restart
    {
        let driver_error = DriverError::new(error.code, error.message);
        outcome.result = match adapter
            .stop(
                &session.public.room_id,
                &session.public.session_id,
                &authority.handle_id,
                &authority.owner_id,
                &authority.lease_token,
            )
            .await
        {
            Ok(()) => Err(ProviderAdapterError::confirmed_stopped(
                driver_error,
                &authority.handle_id,
                &authority.owner_id,
                &authority.lease_token,
            )),
            Err(stop_error) => Err(stop_error),
        };
    }
    let runtime_retained = !outcome.requires_restart
        && match &outcome.result {
            Ok(_) => true,
            Err(error) => !error.effect_uncertain && !error.runtime_stopped,
        };
    adapter
        .retain_prepared_turn_result(&prepared, &outcome.result, runtime_retained)
        .await;
    outcome
}

fn provider_turn_owner_failed(handle_id: &str, owner_id: &str) -> OwnedTurnResult {
    OwnedTurnResult {
        result: Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "provider_turn_owner_failed",
                "The exact provider turn owner ended without a result.",
            ),
            handle_id,
            owner_id,
        )),
        requires_restart: false,
    }
}

struct OwnedTurnResult {
    result: Result<ProviderTurnCompleted, ProviderAdapterError>,
    requires_restart: bool,
}

struct TurnOwnerInput {
    driver_cell: std::sync::Arc<super::runtime_driver::DriverCell>,
    cancellation: CancellationToken,
    session: DurableAgentSession,
    request: ProviderTurnRequest,
    handle_id: String,
    owner_id: String,
    exact_interruption: CancellationToken,
}

async fn run_turn_owner(input: TurnOwnerInput) -> OwnedTurnResult {
    let TurnOwnerInput {
        driver_cell,
        cancellation,
        session,
        request,
        handle_id,
        owner_id,
        exact_interruption,
    } = input;
    let Ok(mut driver) = driver_cell.take().await else {
        return OwnedTurnResult {
            result: Err(ProviderAdapterError::uncertain(
                DriverError::new(
                    "provider_driver_unavailable",
                    "The provider driver is unavailable.",
                ),
                &handle_id,
                &owner_id,
            )),
            requires_restart: false,
        };
    };
    let outcome = AssertUnwindSafe(run_driver_turn(
        driver.as_mut(),
        &cancellation,
        &session,
        &request,
        &handle_id,
        &owner_id,
        &exact_interruption,
    ))
    .catch_unwind()
    .await
    .unwrap_or_else(|_| {
        let mut failed = provider_turn_owner_failed(&handle_id, &owner_id);
        failed.requires_restart = true;
        failed
    });
    driver_cell.put(driver).await;
    outcome
}

async fn run_driver_turn(
    driver: &mut dyn ProviderDriver,
    cancellation: &CancellationToken,
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
    handle_id: &str,
    owner_id: &str,
    exact_interruption: &CancellationToken,
) -> OwnedTurnResult {
    let result = match require_live_driver(driver, cancellation, handle_id, owner_id).await {
        Ok(()) => {
            if request.room_observation.is_some()
                && let Err(error) = driver.begin_room_observation(request)
            {
                Err(ProviderAdapterError::safe(error))
            } else {
                let sent = tokio::select! {
                    biased;
                    () = cancellation.cancelled() => Err(turn_cancelled()),
                    () = exact_interruption.cancelled() => {
                        match driver.interrupt_turn(session, request).await {
                            Ok(()) => Err(turn_interrupted()),
                            Err(error) => Err(error),
                        }
                    },
                    sent = driver.send_turn(session, request) => sent,
                };
                match sent {
                    Ok(completed) => finish_completed_turn(
                        driver, session, request, completed, handle_id, owner_id,
                    ),
                    Err(error) if error.code == "provider_turn_interrupted" => {
                        driver.abort_room_observation();
                        Err(ProviderAdapterError::safe(error))
                    }
                    Err(error) => {
                        driver.abort_room_observation();
                        Err(ProviderAdapterError::uncertain(error, handle_id, owner_id))
                    }
                }
            }
        }
        Err(error) => Err(error),
    };
    OwnedTurnResult {
        result,
        requires_restart: driver.requires_restart(),
    }
}

const fn turn_interrupted() -> DriverError {
    DriverError::new(
        "provider_turn_interrupted",
        "The exact provider turn was interrupted by room authority.",
    )
}

async fn require_live_driver(
    driver: &mut dyn ProviderDriver,
    cancellation: &CancellationToken,
    handle_id: &str,
    owner_id: &str,
) -> Result<(), ProviderAdapterError> {
    if cancellation.is_cancelled() {
        return Err(ProviderAdapterError::uncertain(
            turn_cancelled(),
            handle_id,
            owner_id,
        ));
    }
    match driver.is_alive().await {
        Ok(true) => Ok(()),
        Ok(false) => Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "provider_runtime_exited",
                "The owned provider runtime exited before its assigned turn.",
            ),
            handle_id,
            owner_id,
        )),
        Err(error) => Err(ProviderAdapterError::uncertain(error, handle_id, owner_id)),
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

pub(super) fn validate_request(
    session: &DurableAgentSession,
    request: &ProviderTurnRequest,
) -> Result<(), DriverError> {
    if request.turn_id.is_empty()
        || request.turn_id.len() > MAX_TURN_ID_BYTES
        || request.turn_id.trim() != request.turn_id
        || request.turn_id.chars().any(char::is_control)
        || request.turn_id != session.public.active_turn_id
        || request.turn_generation == 0
        || request.turn_generation != session.turn_generation
        || Uuid::parse_str(&request.execution_id).is_err()
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
        if observation.session_id != session.public.session_id
            || observation.input_up_to_seq <= 0
            || observation.view.chars().count() > MAX_ROOM_VIEW_CHARS
            || observation.view.len() > MAX_ROOM_VIEW_BYTES
            || observation.view.contains('\0')
            || !has_visible_text(&observation.view)
            || !valid_observation_attachments(
                &observation.view,
                &observation.attachment_ids,
                observation.attachment_ingress.is_some(),
            )
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
        || session.runtime_lease_token != runtime.lease_token
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
