#[cfg(not(unix))]
use std::{io, process::Stdio, time::Duration};

use agent_client_protocol::schema::v1::{HttpHeader, McpServer, McpServerHttp};
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
    acp_client::{AcpClient, AcpPermissionPolicy},
    driver::{DriverError, ProviderTurnRequest},
    filesystem::{BoundExecutable, bind_executable_with_children},
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomPortal, RoomPortalError},
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

#[cfg(not(unix))]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) struct AcpRuntime {
    #[cfg(not(unix))]
    child: Box<dyn ChildWrapper>,
    _executable_guard: BoundExecutable,
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    pub(crate) client: AcpClient,
    stderr_task: JoinHandle<()>,
    room_portal: RoomPortal,
}

impl AcpRuntime {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
        arguments: &[String],
        environment: &[(String, String)],
        permission_policy: AcpPermissionPolicy,
    ) -> Result<Self, DriverLaunchError> {
        let executable = bind(session).await?;
        let room_portal = create_room_portal().await?;
        let (process_group, pipes) = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &executable,
            arguments,
            environment,
            std::path::Path::new(&session.workspace),
        )
        .await?;
        let stderr_task = tokio::spawn(drain_stderr(pipes.stderr));
        let client = AcpClient::connect(pipes.stdin, pipes.stdout, permission_policy).await?;
        Ok(Self {
            process_group,
            _executable_guard: executable,
            client,
            stderr_task,
            room_portal,
        })
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        arguments: &[String],
        environment: &[(String, String)],
        permission_policy: AcpPermissionPolicy,
    ) -> Result<Self, DriverLaunchError> {
        #[cfg(not(any(unix, windows)))]
        return Err(DriverError::new(
            "provider_runtime_unsupported",
            "ACP provider processes are unsupported on this platform.",
        )
        .into());
        let executable = bind(session).await?;
        let room_portal = create_room_portal().await?;
        let mut command = CommandWrap::with_new(executable.launch_path(), |command| {
            command
                .args(arguments)
                .current_dir(&session.workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        sanitize_environment(command.command_mut());
        command.command_mut().envs(environment.iter().cloned());
        command.wrap(KillOnDrop);
        #[cfg(windows)]
        command.wrap(JobObject);
        let mut child = command
            .spawn()
            .map_err(|error| DriverLaunchError::safe(spawn_error(&error)))?;
        let stdin = child
            .stdin()
            .take()
            .ok_or_else(|| DriverLaunchError::uncertain(protocol_error()))?;
        let stdout = child
            .stdout()
            .take()
            .ok_or_else(|| DriverLaunchError::uncertain(protocol_error()))?;
        let stderr = child
            .stderr()
            .take()
            .ok_or_else(|| DriverLaunchError::uncertain(protocol_error()))?;
        let stderr_task = tokio::spawn(drain_stderr(stderr));
        let client = AcpClient::connect(stdin, stdout, permission_policy).await?;
        Ok(Self {
            child,
            _executable_guard: executable,
            client,
            stderr_task,
            room_portal,
        })
    }

    pub(crate) fn room_portal_server(&self) -> McpServer {
        McpServer::Http(
            McpServerHttp::new("agentsassemble_room", self.room_portal.endpoint()).headers(vec![
                HttpHeader::new(
                    "Authorization",
                    format!("Bearer {}", self.room_portal.bearer_token()),
                ),
            ]),
        )
    }

    pub(crate) async fn is_alive(&mut self) -> Result<bool, DriverError> {
        if self.client.is_closed() {
            return Ok(false);
        }
        #[cfg(unix)]
        return self.process_group.leader_is_running().await;
        #[cfg(not(unix))]
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| protocol_error())
    }

    pub(crate) async fn stop(&mut self) -> Result<(), DriverError> {
        self.client.shutdown().await;
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
    }

    pub(crate) fn begin_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<(), RoomPortalError> {
        self.room_portal.begin_turn(request)?;
        self.client
            .set_room_observation_active(true)
            .map_err(|_| RoomPortalError::Authority)
    }

    pub(crate) fn finish_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, RoomPortalError> {
        self.client
            .set_room_observation_active(false)
            .map_err(|_| RoomPortalError::Authority)?;
        self.room_portal.finish_turn(request)
    }

    pub(crate) fn abort_observation(&mut self) {
        let _ = self.client.set_room_observation_active(false);
        let _ = self.room_portal.end_observation();
    }
}

impl Drop for AcpRuntime {
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
const fn protocol_error() -> DriverError {
    DriverError::new(
        "provider_protocol_invalid",
        "The ACP provider returned an invalid protocol message.",
    )
}

const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The ACP provider executable no longer matches discovery authority.",
    )
}

#[cfg(not(unix))]
fn spawn_error(error: &io::Error) -> DriverError {
    if error.kind() == io::ErrorKind::NotFound {
        DriverError::new(
            "provider_executable_missing",
            "The ACP provider executable is no longer available.",
        )
    } else {
        DriverError::new(
            "provider_spawn_failed",
            "The ACP provider process could not be started.",
        )
    }
}

#[cfg(not(unix))]
const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_unconfirmed",
        "The ACP provider process shutdown could not be confirmed.",
    )
}

const fn room_portal_unavailable() -> DriverError {
    DriverError::new(
        "room_portal_unavailable",
        "The server-owned provider room portal is unavailable.",
    )
}
