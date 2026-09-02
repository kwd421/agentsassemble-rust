use crate::{
    acp_runtime::AcpRuntime,
    cursor::effective_model,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomPortalError},
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};
use agent_client_protocol::schema::v1::StopReason;
use agentsassemble_domain::DurableAgentSession;

pub(crate) struct CursorAcpDriver {
    runtime: AcpRuntime,
}

impl CursorAcpDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        let runtime =
            AcpRuntime::spawn(session, runtime_lease, guardian, &["acp".to_owned()], &[]).await?;
        Ok(Self { runtime })
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        #[cfg(not(any(unix, windows)))]
        return Err(DriverError::new(
            "provider_runtime_unsupported",
            "Provider processes are unsupported on this platform.",
        )
        .into());
        let runtime = AcpRuntime::spawn(session, &["acp".to_owned()], &[]).await?;
        Ok(Self { runtime })
    }
}

impl ProviderDriver for CursorAcpDriver {
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        true
    }

    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            let model = effective_model(
                &session.public.model,
                &session.public.reasoning_effort,
                &session.public.service_tier,
            )
            .ok_or_else(invalid_profile)?;
            let server = self.runtime.room_portal_server();
            let attached = self
                .runtime
                .client
                .attach(
                    &session.workspace,
                    &session.provider_session_id,
                    server,
                    &model,
                )
                .await?;
            Ok(ProviderSessionAttachment {
                provider_session_id: attached.session_id,
                reused: attached.reused,
                observed_model_id: Some(model),
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(async move {
            let turn = self
                .runtime
                .client
                .prompt(&request.turn_id, &request.input)
                .await?;
            let outcome = match turn.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens | StopReason::MaxTurnRequests
                    if !turn.output.trim().is_empty() =>
                {
                    ProviderTurnOutcome::Message {
                        content: turn.output,
                        target_agent_id: String::new(),
                    }
                }
                StopReason::Refusal => ProviderTurnOutcome::Declined {
                    reason_code: "provider_refusal".to_owned(),
                },
                StopReason::Cancelled => {
                    return Err(DriverError::new(
                        "provider_turn_cancelled",
                        "Cursor ACP cancelled the turn without an authorized interrupt.",
                    ));
                }
                _ => return Err(protocol_error()),
            };
            Ok(ProviderTurnCompleted {
                turn_id: request.turn_id.clone(),
                provider_turn_id: request.turn_id.clone(),
                provider_session_id: Some(turn.session_id),
                outcome,
            })
        })
    }

    fn interrupt_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(self.runtime.client.cancel(&request.turn_id))
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(self.runtime.is_alive())
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.runtime.stop())
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        self.runtime
            .begin_observation(request)
            .map_err(portal_error)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        self.runtime
            .finish_observation(request)
            .map_err(portal_error)
    }

    fn abort_room_observation(&mut self) {
        self.runtime.abort_observation();
    }

    fn requires_restart(&self) -> bool {
        self.runtime.client.requires_restart()
    }
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "Cursor ACP returned an invalid protocol message.",
    )
}

const fn invalid_profile() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The Cursor runtime profile is invalid.",
    )
}

const fn room_portal_unavailable() -> DriverError {
    DriverError::new(
        "room_portal_unavailable",
        "The server-owned provider room portal is unavailable.",
    )
}

const fn portal_error(error: RoomPortalError) -> DriverError {
    match error {
        RoomPortalError::ReceiptMissing => DriverError::new(
            "room_observation_unconfirmed",
            "Cursor did not confirm reading the assigned room observation.",
        ),
        RoomPortalError::OutcomeMissing | RoomPortalError::OutcomeInvalid => DriverError::new(
            "room_portal_publication_missing",
            "Cursor did not stage a valid room publication or decline.",
        ),
        RoomPortalError::Authority | RoomPortalError::Observation | RoomPortalError::Mcp => {
            room_portal_unavailable()
        }
    }
}
