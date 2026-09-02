#[cfg(not(unix))]
use std::{io, process::Stdio};

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
    launch_cleanup,
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomPortal, RoomPortalError},
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

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
        let mut room_portal = create_room_portal().await?;
        let started = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &executable,
            arguments,
            environment,
            std::path::Path::new(&session.workspace),
        )
        .await;
        let (mut process_group, pipes) = match started {
            Ok(started) => started,
            Err(error) => return Err(launch_cleanup::portal(&mut room_portal, error).await),
        };
        let stderr_task = tokio::spawn(drain_stderr(pipes.stderr));
        let client = match AcpClient::connect(pipes.stdin, pipes.stdout, permission_policy).await {
            Ok(client) => client,
            Err(error) => {
                let process = process_group.stop().await;
                stderr_task.abort();
                let _ = stderr_task.await;
                return Err(
                    launch_cleanup::owned_and_portal(&mut room_portal, process, error).await,
                );
            }
        };
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
        let mut room_portal = create_room_portal().await?;
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
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(error) => {
                let failure = DriverLaunchError::safe(spawn_error(&error));
                return Err(launch_cleanup::portal(&mut room_portal, failure).await);
            }
        };
        let Some(stdin) = child.stdin().take() else {
            let process = stop_failed_child(child.as_mut()).await;
            let failure = DriverLaunchError::safe(protocol_error());
            return Err(launch_cleanup::owned_and_portal(&mut room_portal, process, failure).await);
        };
        let Some(stdout) = child.stdout().take() else {
            let process = stop_failed_child(child.as_mut()).await;
            let failure = DriverLaunchError::safe(protocol_error());
            return Err(launch_cleanup::owned_and_portal(&mut room_portal, process, failure).await);
        };
        let Some(stderr) = child.stderr().take() else {
            let process = stop_failed_child(child.as_mut()).await;
            let failure = DriverLaunchError::safe(protocol_error());
            return Err(launch_cleanup::owned_and_portal(&mut room_portal, process, failure).await);
        };
        let stderr_task = tokio::spawn(drain_stderr(stderr));
        let client = match AcpClient::connect(stdin, stdout, permission_policy).await {
            Ok(client) => client,
            Err(error) => {
                let process = stop_failed_child(child.as_mut()).await;
                stderr_task.abort();
                let _ = stderr_task.await;
                return Err(
                    launch_cleanup::owned_and_portal(&mut room_portal, process, error).await,
                );
            }
        };
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
        let process = self.process_group.stop().await;
        #[cfg(not(unix))]
        let process = stop_failed_child(self.child.as_mut()).await;
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;
        let portal = self
            .room_portal
            .shutdown()
            .await
            .map_err(|_| room_portal_unavailable());
        process.and(portal)
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

    pub(crate) fn abort_observation(&mut self) -> Result<(), RoomPortalError> {
        let protocol = self
            .client
            .set_room_observation_active(false)
            .map_err(|_| RoomPortalError::Authority);
        let portal = self.room_portal.end_observation();
        protocol.and(portal)
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

#[cfg(not(unix))]
async fn stop_failed_child(child: &mut dyn ChildWrapper) -> Result<(), DriverError> {
    crate::process::stop_child(child)
        .await
        .map_err(|_| stop_error())
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
