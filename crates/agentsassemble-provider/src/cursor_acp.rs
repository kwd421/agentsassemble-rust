use crate::{
    acp_client::AcpPermissionPolicy,
    acp_runtime::AcpRuntime,
    cursor::effective_model,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    launch_error::DriverLaunchError,
    room_portal::ProviderTurnOutcome,
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};
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
        let runtime = AcpRuntime::spawn(
            session,
            bind(session).await?,
            runtime_lease,
            guardian,
            &["acp".to_owned()],
            &[],
            AcpPermissionPolicy::Reject,
        )
        .await?;
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
        let runtime = AcpRuntime::spawn(
            session,
            bind(session).await?,
            &["acp".to_owned()],
            &[],
            AcpPermissionPolicy::Reject,
        )
        .await?;
        Ok(Self { runtime })
    }
}

impl ProviderDriver for CursorAcpDriver {
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        crate::registration::CURSOR_PROVIDER
            .turn_interrupt
            .retains_runtime()
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
        session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(
            self.runtime
                .client
                .prompt(&session.public.session_id, request),
        )
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

    fn begin_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<(), DriverError>> {
        Box::pin(async move {
            self.runtime
                .begin_observation(request)
                .map_err(DriverError::from)
        })
    }

    fn finish_room_observation<'a>(
        &'a mut self,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnOutcome, DriverError>> {
        Box::pin(async move {
            self.runtime
                .finish_observation(request)
                .map_err(DriverError::from)
        })
    }

    fn abort_room_observation(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move { self.runtime.abort_observation().map_err(DriverError::from) })
    }

    fn requires_restart(&self) -> bool {
        self.runtime.client.requires_restart()
    }
}

const fn invalid_profile() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The Cursor runtime profile is invalid.",
    )
}

async fn bind(
    session: &DurableAgentSession,
) -> Result<crate::filesystem::BoundExecutable, DriverLaunchError> {
    crate::filesystem::bind_cursor_executable(
        session.executable.clone(),
        session.executable_identity.clone(),
    )
    .await
    .map_err(|_| {
        DriverLaunchError::safe(DriverError::new(
            "provider_executable_changed",
            "The Cursor executable package no longer matches discovery authority.",
        ))
    })
}
