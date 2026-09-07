use std::{future::Future, pin::Pin};

use agentsassemble_domain::DurableAgentSession;
use thiserror::Error;

use crate::{
    room_attachment::ProviderAttachmentReadIngress,
    room_portal::{ProviderRoomToolIngress, ProviderTurnOutcome, RoomPortalError},
};

pub(crate) const ROOM_PORTAL_UNAVAILABLE: DriverError = DriverError::new(
    "room_portal_unavailable",
    "The server-owned provider room portal is unavailable.",
);

pub(crate) type DriverFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub(crate) trait ProviderDriver: Send {
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        false
    }
    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>>;
    fn send_turn<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>>;
    fn interrupt_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        _request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async {
            Err(DriverError::new(
                "provider_turn_interrupt_unsupported",
                "The provider does not support exact-turn interruption.",
            ))
        })
    }
    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>>;
    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>>;
    fn begin_room_observation(
        &mut self,
        _request: &ProviderTurnRequest,
    ) -> Result<(), DriverError> {
        Err(ROOM_PORTAL_UNAVAILABLE)
    }
    fn finish_room_observation(
        &mut self,
        _request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        Err(ROOM_PORTAL_UNAVAILABLE)
    }
    fn abort_room_observation(&mut self) -> Result<(), DriverError> {
        Ok(())
    }
    fn requires_restart(&self) -> bool {
        false
    }
    fn attachment_replay_is_safe(&self) -> bool {
        true
    }
    fn turn_failure_effect_uncertain(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProviderSessionAttachment {
    pub(crate) provider_session_id: String,
    pub(crate) reused: bool,
    pub(crate) observed_model_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{message}")]
pub(crate) struct DriverError {
    pub(crate) code: &'static str,
    pub(crate) message: &'static str,
}

impl DriverError {
    pub(crate) const fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

impl From<RoomPortalError> for DriverError {
    fn from(error: RoomPortalError) -> Self {
        match error {
            RoomPortalError::ReceiptMissing => Self::new(
                "room_observation_unconfirmed",
                "The provider did not confirm reading the assigned room observation.",
            ),
            RoomPortalError::OutcomeMissing | RoomPortalError::OutcomeInvalid => Self::new(
                "room_portal_publication_missing",
                "The provider did not stage a valid room publication or decline.",
            ),
            RoomPortalError::Authority | RoomPortalError::Observation | RoomPortalError::Mcp => {
                ROOM_PORTAL_UNAVAILABLE
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnRequest {
    pub turn_id: String,
    pub turn_generation: u64,
    pub execution_id: String,
    pub input: String,
    pub room_observation: Option<ProviderRoomObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRoomObservation {
    pub session_id: String,
    pub input_up_to_seq: i64,
    pub view: String,
    pub attachment_ids: Vec<String>,
    pub attachment_ingress: Option<ProviderAttachmentReadIngress>,
    pub allowed_agent_ids: Vec<String>,
    pub tabletop_tools: bool,
    pub room_tool_ingress: Option<ProviderRoomToolIngress>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderTurnCompleted {
    pub turn_id: String,
    pub provider_turn_id: String,
    pub provider_session_id: Option<String>,
    pub outcome: ProviderTurnOutcome,
}
