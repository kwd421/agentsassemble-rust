use std::path::PathBuf;

use agentsassemble_domain::DurableAgentSession;

use crate::{
    antigravity_hook::AntigravityHookRegistration,
    antigravity_transport::AntigravityTerminal,
    catalog::{ANTIGRAVITY_NATIVE_RECEIPT_ERROR_CODE, ANTIGRAVITY_NATIVE_RECEIPT_ERROR_MESSAGE},
    filesystem::{BoundExecutable, bind_executable},
    launch_error::DriverLaunchError,
    room_portal::{RoomPortal, RoomPortalError},
    room_portal_terminal::RoomPortalTerminalHelper,
    runtime::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};

const MAX_PROVIDER_SESSION_ID_BYTES: usize = 200;
const PENDING_SESSION_PREFIX: &str = "pending-antigravity-";

pub(super) fn command_arguments(session: &DurableAgentSession) -> Result<Vec<String>, DriverError> {
    let model = session.public.model.trim();
    let effort = session.public.reasoning_effort.trim().to_ascii_lowercase();
    if model.is_empty()
        || model != session.public.model
        || model.chars().any(char::is_control)
        || !matches!(effort.as_str(), "" | "low" | "medium" | "high")
    {
        return Err(profile_error());
    }
    let effective_model =
        if effort.is_empty() || model.to_ascii_lowercase().ends_with(&format!("-{effort}")) {
            model.to_owned()
        } else {
            format!("{model}-{effort}")
        };
    let mut arguments = vec!["--model".to_owned(), effective_model];
    match session.public.permission_mode.as_str() {
        "workspace_write" => arguments.extend(["--mode".to_owned(), "accept-edits".to_owned()]),
        "meeting_read_only" => arguments.push("--sandbox".to_owned()),
        _ => return Err(profile_error()),
    }
    let provider_session_id = clean_identifier(&session.provider_session_id);
    if !session.provider_session_id.is_empty() && provider_session_id.is_empty() {
        return Err(profile_error());
    }
    if !provider_session_id.is_empty() && !provider_session_id.starts_with(PENDING_SESSION_PREFIX) {
        arguments.extend(["--conversation".to_owned(), provider_session_id]);
    }
    Ok(arguments)
}

struct PreparedAntigravity {
    arguments: Vec<String>,
    workspace: PathBuf,
    executable: BoundExecutable,
    room_portal: RoomPortal,
}

async fn prepare(session: &DurableAgentSession) -> Result<PreparedAntigravity, DriverLaunchError> {
    let arguments = command_arguments(session)?;
    let workspace = PathBuf::from(&session.workspace);
    let executable = bind_executable(
        session.executable.clone(),
        session.executable_identity.clone(),
    )
    .await
    .map_err(|_| executable_error())?;
    let room_portal = RoomPortal::create().await.map_err(portal_driver_error)?;
    Ok(PreparedAntigravity {
        arguments,
        workspace,
        executable,
        room_portal,
    })
}

pub(crate) struct AntigravityDriver {
    terminal: Box<dyn AntigravityTerminal>,
    _room_portal: RoomPortal,
    terminal_helper: Option<RoomPortalTerminalHelper>,
    hook: Option<AntigravityHookRegistration>,
}

impl AntigravityDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
    ) -> Result<Self, DriverLaunchError> {
        require_native_receipt()?;
        let PreparedAntigravity {
            arguments,
            workspace,
            executable,
            room_portal,
        } = prepare(session).await?;
        let terminal_helper = room_portal
            .create_terminal_helper(guardian)
            .map_err(portal_driver_error)?;
        let hook = AntigravityHookRegistration::register(
            &workspace,
            terminal_helper.hook_command(),
            terminal_helper.hook_executable_owner(),
        )?;
        let mut environment = terminal_helper.provider_environment();
        environment.extend([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("COLUMNS".to_owned(), "120".to_owned()),
            ("LINES".to_owned(), "40".to_owned()),
        ]);
        let terminal = crate::antigravity_unix::spawn_terminal(
            runtime_lease,
            guardian,
            executable,
            &arguments,
            &environment,
            &workspace,
        )
        .await?;
        Ok(Self::from_parts(
            terminal,
            room_portal,
            terminal_helper,
            hook,
        ))
    }

    #[cfg(windows)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        companion: &BoundExecutable,
    ) -> Result<Self, DriverLaunchError> {
        require_native_receipt()?;
        let PreparedAntigravity {
            arguments,
            workspace,
            executable,
            room_portal,
        } = prepare(session).await?;
        let terminal_helper = room_portal
            .create_terminal_helper(companion)
            .map_err(portal_driver_error)?;
        let hook = AntigravityHookRegistration::register(
            &workspace,
            terminal_helper.hook_command(),
            terminal_helper.hook_executable_owner(),
        )?;
        let mut environment = terminal_helper.provider_environment();
        environment.extend([
            ("TERM".to_owned(), "xterm-256color".to_owned()),
            ("COLORTERM".to_owned(), "truecolor".to_owned()),
            ("COLUMNS".to_owned(), "120".to_owned()),
            ("LINES".to_owned(), "40".to_owned()),
        ]);
        let terminal = crate::antigravity_windows::spawn_terminal(
            executable,
            &arguments,
            &environment,
            &workspace,
        )?;
        Ok(Self::from_parts(
            terminal,
            room_portal,
            terminal_helper,
            hook,
        ))
    }

    fn from_parts(
        terminal: Box<dyn AntigravityTerminal>,
        room_portal: RoomPortal,
        terminal_helper: RoomPortalTerminalHelper,
        hook: AntigravityHookRegistration,
    ) -> Self {
        Self {
            terminal,
            _room_portal: room_portal,
            terminal_helper: Some(terminal_helper),
            hook: Some(hook),
        }
    }

    async fn stop_process(&mut self) -> Result<(), DriverError> {
        self.terminal.stop().await?;
        self.hook.take();
        self.terminal_helper.take();
        Ok(())
    }
}

impl ProviderDriver for AntigravityDriver {
    fn attach_session<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async { Err(native_receipt_unavailable()) })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        _request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(async { Err(native_receipt_unavailable()) })
    }

    fn is_alive(&mut self) -> DriverFuture<'_, Result<bool, DriverError>> {
        self.terminal.is_alive()
    }

    fn stop(&mut self) -> DriverFuture<'_, Result<(), DriverError>> {
        Box::pin(self.stop_process())
    }

    fn requires_restart(&self) -> bool {
        true
    }
}

impl Drop for AntigravityDriver {
    fn drop(&mut self) {
        self.terminal.request_stop();
    }
}

fn portal_driver_error(_error: RoomPortalError) -> DriverError {
    DriverError::new(
        "room_portal_unavailable",
        "The Antigravity room portal is unavailable.",
    )
}

const fn executable_error() -> DriverError {
    DriverError::new(
        "provider_executable_changed",
        "The selected Antigravity executable authority changed.",
    )
}

const fn native_receipt_unavailable() -> DriverError {
    DriverError::new(
        ANTIGRAVITY_NATIVE_RECEIPT_ERROR_CODE,
        ANTIGRAVITY_NATIVE_RECEIPT_ERROR_MESSAGE,
    )
}

fn require_native_receipt() -> Result<(), DriverLaunchError> {
    Err(DriverLaunchError::safe(native_receipt_unavailable()))
}

fn clean_identifier(value: &str) -> String {
    let value = value.trim();
    let mut components = std::path::Path::new(value).components();
    if value.is_empty()
        || value.len() > MAX_PROVIDER_SESSION_ID_BYTES
        || value.chars().any(char::is_control)
        || value == "--last"
        || !matches!(components.next(), Some(std::path::Component::Normal(_)))
        || components.next().is_some()
    {
        String::new()
    } else {
        value.to_owned()
    }
}

const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The stored Antigravity runtime profile is invalid.",
    )
}

#[cfg(test)]
#[path = "antigravity_tests.rs"]
mod tests;
