use std::path::Path;

use agentsassemble_domain::DurableAgentSession;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite},
    task::JoinHandle,
};

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
use crate::{
    guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease, unix_custody::UnixProcessCustody,
};

type Client =
    ClaudeSdkClient<Box<dyn AsyncWrite + Send + Unpin>, Box<dyn AsyncRead + Send + Unpin>>;

pub(crate) struct ClaudeSdkRuntime {
    process_group: UnixProcessCustody,
    _node_guard: BoundExecutable,
    _claude_guard: BoundExecutable,
    _private_claude: PrivateExecutable,
    _sdk_bundle: PrivateClaudeSdkBundle,
    client: Client,
    stderr_task: JoinHandle<()>,
    room_portal: RoomPortal,
}

impl ClaudeSdkRuntime {
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<(Self, ClaudeSdkAttachment), DriverLaunchError> {
        let (node, claude, private_claude, sdk_bundle) = bind_runtime(session).await?;
        let room_portal = create_room_portal().await?;
        let arguments = arguments(&sdk_bundle, &private_claude)?;
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
        self.process_group.leader_is_running().await
    }

    pub(crate) async fn stop(&mut self) -> Result<(), DriverError> {
        let protocol = self.client.shutdown().await;
        let process = self.process_group.stop().await;
        self.stderr_task.abort();
        let _ = (&mut self.stderr_task).await;
        protocol.and(process)
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

    pub(crate) fn abort_observation(&mut self) {
        let _ = self.room_portal.end_observation();
    }

    pub(crate) const fn requires_restart(&self) -> bool {
        self.client.requires_restart()
    }
}

impl Drop for ClaudeSdkRuntime {
    fn drop(&mut self) {
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
        PrivateExecutable,
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
    let companion_name = Path::new(&session.executable)
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| DriverLaunchError::safe(executable_error()))?;
    let private_claude = claude
        .stage_private_companion(companion_name)
        .map_err(|_| DriverLaunchError::safe(executable_error()))?;
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
    claude: &PrivateExecutable,
) -> Result<Vec<String>, DriverLaunchError> {
    [&bundle.bridge, &bundle.sdk, claude.path()]
        .into_iter()
        .map(|path| {
            path.to_str()
                .map(ToOwned::to_owned)
                .ok_or_else(|| DriverLaunchError::safe(sdk_error()))
        })
        .chain(std::iter::once(Ok("session".to_owned())))
        .collect()
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

const fn sdk_error() -> DriverError {
    DriverError::new("provider_sdk_missing", "Claude Agent SDK is unavailable.")
}

const fn room_portal_unavailable() -> DriverError {
    DriverError::new("room_portal_unavailable", "The room portal is unavailable.")
}
