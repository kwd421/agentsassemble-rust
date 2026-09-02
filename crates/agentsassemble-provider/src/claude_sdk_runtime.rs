use std::path::Path;
#[cfg(windows)]
use std::{process::Stdio, time::Duration};

use agentsassemble_domain::DurableAgentSession;
#[cfg(windows)]
use process_wrap::tokio::{ChildWrapper, CommandWrap, JobObject, KillOnDrop};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    task::JoinHandle,
};

#[cfg(windows)]
use crate::process::sanitize_environment;
use crate::{
    claude_sdk_assets::PrivateClaudeSdkBundle,
    claude_sdk_client::{ClaudeSdkAttachment, ClaudeSdkClient, ClaudeSdkTurn},
    driver::{DriverError, ProviderTurnRequest},
    filesystem::{
        BoundExecutable, PrivateExecutable, bind_executable, bind_executable_with_children,
        resolve_executable,
    },
    launch_error::DriverLaunchError,
    room_portal::{ProviderTurnOutcome, RoomPortal, RoomPortalError},
};
#[cfg(unix)]
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

#[cfg(windows)]
const STOP_TIMEOUT: Duration = Duration::from_secs(5);

type Client =
    ClaudeSdkClient<Box<dyn AsyncWrite + Send + Unpin>, Box<dyn AsyncRead + Send + Unpin>>;

pub(crate) struct ClaudeSdkRuntime {
    #[cfg(unix)]
    process_group: UnixProcessCustody,
    #[cfg(windows)]
    child: Box<dyn ChildWrapper>,
    _node_guard: BoundExecutable,
    _claude_guard: BoundExecutable,
    _private_claude: Option<PrivateExecutable>,
    _sdk_bundle: PrivateClaudeSdkBundle,
    client: Client,
    stderr_task: JoinHandle<()>,
    room_portal: RoomPortal,
}

impl ClaudeSdkRuntime {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<(Self, ClaudeSdkAttachment), DriverLaunchError> {
        let (node, claude, private_claude, sdk_bundle) = bind_runtime(session).await?;
        let room_portal = create_room_portal().await?;
        let arguments = arguments(
            &sdk_bundle,
            child_executable_path(&claude, private_claude.as_ref()),
        )?;
        let (process_group, pipes) = UnixProcessCustody::start_with_children(
            runtime_lease,
            guardian,
            &node,
            &arguments,
            &[],
            Path::new(&session.workspace),
        )
        .await?;
        let stderr_task = tokio::spawn(drain_stderr(pipes.stderr));
        let (client, attachment) =
            connect_client(pipes.stdin, pipes.stdout, session, &room_portal).await?;
        Ok((
            Self {
                process_group,
                _node_guard: node,
                _claude_guard: claude,
                _private_claude: private_claude,
                _sdk_bundle: sdk_bundle,
                client,
                stderr_task,
                room_portal,
            },
            attachment,
        ))
    }

    #[cfg(windows)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
    ) -> Result<(Self, ClaudeSdkAttachment), DriverLaunchError> {
        let (node, claude, private_claude, sdk_bundle) = bind_runtime(session).await?;
        let room_portal = create_room_portal().await?;
        let arguments = arguments(
            &sdk_bundle,
            child_executable_path(&claude, private_claude.as_ref()),
        )?;
        let mut command = CommandWrap::with_new(node.launch_path(), |command| {
            command
                .args(&arguments)
                .current_dir(&session.workspace)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        });
        sanitize_environment(command.command_mut());
        command.wrap(KillOnDrop);
        command.wrap(JobObject);
        let mut child = command
            .spawn()
            .map_err(|_| DriverLaunchError::safe(spawn_error()))?;
        drop(command);
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
        let (client, attachment) = connect_client(stdin, stdout, session, &room_portal).await?;
        Ok((
            Self {
                child,
                _node_guard: node,
                _claude_guard: claude,
                _private_claude: private_claude,
                _sdk_bundle: sdk_bundle,
                client,
                stderr_task,
                room_portal,
            },
            attachment,
        ))
    }

    pub(crate) async fn turn(
        &mut self,
        turn_id: &str,
        input: &str,
    ) -> Result<ClaudeSdkTurn, DriverError> {
        self.client.turn(turn_id, input).await
    }

    pub(crate) async fn is_alive(&mut self) -> Result<bool, DriverError> {
        if self.client.requires_restart() {
            return Ok(false);
        }
        #[cfg(unix)]
        return self.process_group.leader_is_running().await;
        #[cfg(windows)]
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(|_| protocol_error())
    }

    pub(crate) async fn stop(&mut self) -> Result<(), DriverError> {
        let protocol = self.client.shutdown().await;
        #[cfg(unix)]
        let process = self.process_group.stop().await;
        #[cfg(windows)]
        let process = tokio::time::timeout(STOP_TIMEOUT, Box::into_pin(self.child.kill()))
            .await
            .map_err(|_| stop_error())?
            .map_err(|_| stop_error());
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;
        let portal = self
            .room_portal
            .shutdown()
            .await
            .map_err(|_| room_portal_unavailable());
        protocol.and(process).and(portal)
    }

    pub(crate) fn begin_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<(), RoomPortalError> {
        self.room_portal.begin_turn(request)
    }

    pub(crate) fn finish_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, RoomPortalError> {
        self.room_portal.finish_turn(request)
    }

    pub(crate) fn abort_observation(&mut self) -> Result<(), RoomPortalError> {
        self.room_portal.end_observation()
    }

    pub(crate) const fn requires_restart(&self) -> bool {
        self.client.requires_restart()
    }
}

impl Drop for ClaudeSdkRuntime {
    fn drop(&mut self) {
        #[cfg(unix)]
        self.process_group.request_stop();
        self.stderr_task.abort();
    }
}

async fn bind_runtime(
    session: &DurableAgentSession,
) -> Result<
    (
        BoundExecutable,
        BoundExecutable,
        Option<PrivateExecutable>,
        PrivateClaudeSdkBundle,
    ),
    DriverLaunchError,
> {
    let claude = bind_executable(
        session.executable.clone(),
        session.executable_identity.clone(),
    )
    .await
    .map_err(|_| DriverLaunchError::safe(executable_error()))?;
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let private_claude = {
        let companion_name = Path::new(&session.executable)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| DriverLaunchError::safe(executable_error()))?;
        Some(
            claude
                .stage_private_companion(companion_name)
                .map_err(|_| DriverLaunchError::safe(executable_error()))?,
        )
    };
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let private_claude: Option<PrivateExecutable> = None;
    let (node_path, node_identity) = resolve_executable("node")
        .await
        .map_err(|_| DriverLaunchError::safe(node_error()))?
        .ok_or_else(|| DriverLaunchError::safe(node_error()))?;
    let node = bind_executable_with_children(node_path, node_identity)
        .await
        .map_err(|_| DriverLaunchError::safe(node_error()))?;
    let sdk_bundle = PrivateClaudeSdkBundle::stage()
        .await
        .map_err(|_| DriverLaunchError::safe(sdk_error()))?;
    Ok((node, claude, private_claude, sdk_bundle))
}

fn arguments(
    bundle: &PrivateClaudeSdkBundle,
    claude: &Path,
) -> Result<Vec<String>, DriverLaunchError> {
    [&bundle.bridge, &bundle.sdk, claude]
        .into_iter()
        .map(|path| {
            path.to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| DriverLaunchError::safe(sdk_error()))
        })
        .chain(std::iter::once(Ok("session".to_owned())))
        .collect()
}

fn child_executable_path<'a>(
    bound: &'a BoundExecutable,
    private: Option<&'a PrivateExecutable>,
) -> &'a Path {
    private.map_or_else(|| Path::new(bound.launch_path()), PrivateExecutable::path)
}

async fn connect_client<I, O>(
    input: I,
    output: O,
    session: &DurableAgentSession,
    portal: &RoomPortal,
) -> Result<(Client, ClaudeSdkAttachment), DriverLaunchError>
where
    I: tokio::io::AsyncWrite + Send + Unpin + 'static,
    O: tokio::io::AsyncRead + Send + Unpin + 'static,
{
    ClaudeSdkClient::connect(
        Box::new(input) as Box<dyn AsyncWrite + Send + Unpin>,
        Box::new(output) as Box<dyn AsyncRead + Send + Unpin>,
        &session.workspace,
        &session.public.model,
        &session.public.reasoning_effort,
        &session.public.service_tier,
        &session.public.permission_mode,
        &session.provider_session_id,
        portal.endpoint().to_owned(),
        portal.bearer_token(),
    )
    .await
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

const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The Claude executable no longer matches discovery authority.",
    )
}

const fn node_error() -> DriverError {
    DriverError::new("provider_sdk_host_missing", "Node.js is unavailable.")
}

#[cfg(windows)]
const fn spawn_error() -> DriverError {
    DriverError::new(
        "provider_sdk_host_failed",
        "The Claude Agent SDK host could not start.",
    )
}

#[cfg(windows)]
const fn stop_error() -> DriverError {
    DriverError::new(
        "provider_stop_failed",
        "The Claude Agent SDK process did not stop cleanly.",
    )
}

const fn sdk_error() -> DriverError {
    DriverError::new("provider_sdk_missing", "Claude Agent SDK is unavailable.")
}

const fn room_portal_unavailable() -> DriverError {
    DriverError::new("room_portal_unavailable", "The room portal is unavailable.")
}
