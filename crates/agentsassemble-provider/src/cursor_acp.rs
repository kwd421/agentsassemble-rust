#[cfg(not(unix))]
use std::{io, process::Stdio, time::Duration};

use agent_client_protocol::schema::v1::{HttpHeader, McpServer, McpServerHttp, StopReason};
use agentsassemble_domain::DurableAgentSession;
#[cfg(windows)]
use process_wrap::tokio::JobObject;
#[cfg(not(unix))]
use process_wrap::tokio::{ChildWrapper, CommandWrap, KillOnDrop};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    task::JoinHandle,
};

#[cfg(not(unix))]
use crate::process::sanitize_environment;
use crate::{
    cursor::effective_model,
    cursor_acp_protocol::CursorAcpClient,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    filesystem::{BoundExecutable, bind_executable_with_children},
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomObservationStart, RoomPortal, RoomPortalError},
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

#[cfg(not(unix))]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct CursorAcpDriver {
    #[cfg(not(unix))]
    child: Box<dyn ChildWrapper>,
    _executable_guard: BoundExecutable,
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    protocol: CursorAcpClient,
    stderr_task: JoinHandle<()>,
    room_portal: RoomPortal,
}

impl CursorAcpDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        let executable = bind(session).await?;
        let room_portal = create_room_portal().await?;
        let (process_group, pipes) = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &executable,
            &["acp".to_owned()],
            &[],
            std::path::Path::new(&session.workspace),
        )
        .await?;
        let stderr_task = tokio::spawn(drain_stderr(pipes.stderr));
        let protocol = CursorAcpClient::connect(pipes.stdin, pipes.stdout).await?;
        Ok(Self {
            process_group,
            _executable_guard: executable,
            protocol,
            stderr_task,
            room_portal,
        })
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(session: &DurableAgentSession) -> Result<Self, DriverLaunchError> {
        #[cfg(not(any(unix, windows)))]
        return Err(DriverError::new(
            "provider_runtime_unsupported",
            "Provider processes are unsupported on this platform.",
        )
        .into());
        let executable = bind(session).await?;
        let room_portal = create_room_portal().await?;
        let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
            command
                .arg("acp")
                .current_dir(&session.workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        sanitize_environment(command.command_mut());
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        command.wrap(JobObject);
        let mut child = command.spawn().map_err(|error| spawn_error(&error))?;
        let stdin = child.stdin().take().ok_or_else(protocol_error)?;
        let stdout = child.stdout().take().ok_or_else(protocol_error)?;
        let stderr = child.stderr().take().ok_or_else(protocol_error)?;
        let stderr_task = tokio::spawn(drain_stderr(stderr));
        let protocol = CursorAcpClient::connect(stdin, stdout).await?;
        Ok(Self {
            child,
            _executable_guard: executable,
            protocol,
            stderr_task,
            room_portal,
        })
    }

    fn room_portal_server(&self) -> McpServer {
        McpServer::Http(
            McpServerHttp::new("agentsassemble_room", self.room_portal.endpoint()).headers(vec![
                HttpHeader::new(
                    "Authorization",
                    format!("Bearer {}", self.room_portal.bearer_token()),
                ),
            ]),
        )
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
            let server = self.room_portal_server();
            let attached = self
                .protocol
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
                .protocol
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
        Box::pin(self.protocol.cancel(&request.turn_id))
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        Box::pin(async move {
            if self.protocol.is_closed() {
                return Ok(false);
            }
            #[cfg(unix)]
            return self.process_group.leader_is_running().await;
            #[cfg(not(unix))]
            self.child
                .try_wait()
                .map(|status| status.is_none())
                .map_err(|_| protocol_error())
        })
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(async move {
            self.protocol.shutdown().await;
            #[cfg(unix)]
            self.process_group.stop().await?;
            #[cfg(not(unix))]
            tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(self.child.kill()))
                .await
                .map_err(|_| stop_error())?
                .map_err(|_| stop_error())?;
            self.stderr_task.abort();
            let _ = (&mut self.stderr_task).await;
            Ok(())
        })
    }

    fn begin_room_observation(&mut self, request: &ProviderTurnRequest) -> Result<(), DriverError> {
        let observation = request
            .room_observation
            .as_ref()
            .ok_or_else(room_portal_unavailable)?;
        self.room_portal
            .begin_observation(RoomObservationStart {
                session_id: &observation.session_id,
                turn_id: &request.turn_id,
                input_up_to_seq: observation.input_up_to_seq,
                durable_turn_generation: request.turn_generation,
                execution_id: &request.execution_id,
                room_view: &observation.view,
                attachment_ids: &observation.attachment_ids,
                attachment_ingress: observation.attachment_ingress.clone(),
                allowed_agent_ids: &observation.allowed_agent_ids,
                tabletop_tools: observation.tabletop_tools,
                tool_ingress: observation.room_tool_ingress.clone(),
            })
            .map_err(portal_error)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        let observation = request
            .room_observation
            .as_ref()
            .ok_or_else(room_portal_unavailable)?;
        self.room_portal
            .finish_observation(&request.turn_id, observation.input_up_to_seq)
            .map_err(portal_error)
    }

    fn abort_room_observation(&mut self) {
        let _ = self.room_portal.end_observation();
    }

    fn requires_restart(&self) -> bool {
        self.protocol.requires_restart()
    }
}

impl Drop for CursorAcpDriver {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.process_group.request_stop();
        self.stderr_task.abort();
    }
}

async fn bind(session: &DurableAgentSession) -> Result<BoundExecutable, DriverLaunchError> {
    bind_executable_with_children(
        session.executable.clone(),
        session.executable_identity.clone(),
    )
    .await
    .map_err(|_| DriverLaunchError::safe(executable_error()))
}

async fn create_room_portal() -> Result<RoomPortal, DriverLaunchError> {
    RoomPortal::create()
        .await
        .map_err(|_| DriverLaunchError::safe(room_portal_unavailable()))
}

async fn drain_stderr(mut stderr: impl AsyncRead + Unpin) {
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
    }
}

#[cfg(not(unix))]
fn spawn_error(error: &io::Error) -> DriverLaunchError {
    let error = if error.kind() == io::ErrorKind::NotFound {
        DriverError::new(
            "provider_executable_missing",
            "The Cursor executable is no longer available.",
        )
    } else {
        DriverError::new(
            "provider_spawn_failed",
            "The Cursor ACP process could not be started.",
        )
    };
    DriverLaunchError::safe(error)
}

const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "Cursor ACP returned an invalid protocol message.",
    )
}

const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The Cursor executable no longer matches discovery authority.",
    )
}

#[cfg(not(unix))]
const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_unconfirmed",
        "The Cursor ACP process shutdown could not be confirmed.",
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
