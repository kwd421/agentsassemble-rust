use std::{
    env,
    path::{Path, PathBuf},
};

use agentsassemble_domain::DurableAgentSession;
use sha2::{Digest, Sha256};

use crate::{
    acp_client::AcpPermissionPolicy,
    acp_runtime::AcpRuntime,
    driver::{
        DriverError, DriverFuture, ProviderDriver, ProviderSessionAttachment,
        ProviderTurnCompleted, ProviderTurnRequest,
    },
    grok::valid_model_id,
    launch_error::DriverLaunchError,
    room_portal::ProviderTurnOutcome,
};
#[cfg(unix)]
use crate::{guardian::GuardianLaunch, runtime_lease::HeldRuntimeLease};

pub(crate) struct GrokAcpDriver {
    runtime: AcpRuntime,
}

impl GrokAcpDriver {
    #[cfg(unix)]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        runtime_lease: &HeldRuntimeLease,
        guardian: &GuardianLaunch,
        state_root: &Path,
    ) -> Result<Self, DriverLaunchError> {
        let arguments = arguments(session)?;
        let environment = environment(state_root, session).await?;
        let runtime = AcpRuntime::spawn(
            session,
            runtime_lease,
            guardian,
            &arguments,
            &environment,
            AcpPermissionPolicy::RoomTools,
        )
        .await?;
        Ok(Self { runtime })
    }

    #[cfg(not(unix))]
    pub(crate) async fn spawn(
        session: &DurableAgentSession,
        state_root: &Path,
    ) -> Result<Self, DriverLaunchError> {
        let arguments = arguments(session)?;
        let environment = environment(state_root, session).await?;
        let runtime = AcpRuntime::spawn(
            session,
            &arguments,
            &environment,
            AcpPermissionPolicy::RoomTools,
        )
        .await?;
        Ok(Self { runtime })
    }
}

impl ProviderDriver for GrokAcpDriver {
    fn retains_runtime_after_turn_interrupt(&self) -> bool {
        true
    }

    fn attach_session<'a>(
        &'a mut self,
        session: &'a DurableAgentSession,
    ) -> DriverFuture<'a, Result<ProviderSessionAttachment, DriverError>> {
        Box::pin(async move {
            validate_profile(session)?;
            let server = self.runtime.room_portal_server();
            let attached = self
                .runtime
                .client
                .attach_process_model(
                    &session.workspace,
                    &session.provider_session_id,
                    server,
                    &session.public.model,
                )
                .await?;
            Ok(ProviderSessionAttachment {
                provider_session_id: attached.session_id,
                reused: attached.reused,
                observed_model_id: Some(session.public.model.clone()),
            })
        })
    }

    fn send_turn<'a>(
        &'a mut self,
        _session: &'a DurableAgentSession,
        request: &'a ProviderTurnRequest,
    ) -> DriverFuture<'a, Result<ProviderTurnCompleted, DriverError>> {
        Box::pin(self.runtime.client.prompt(
            &request.turn_id,
            &request.input,
            request.room_observation.is_some(),
        ))
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
            .map_err(DriverError::from)
    }

    fn finish_room_observation(
        &mut self,
        request: &ProviderTurnRequest,
    ) -> Result<ProviderTurnOutcome, DriverError> {
        self.runtime
            .finish_observation(request)
            .map_err(DriverError::from)
    }

    fn abort_room_observation(&mut self) -> Result<(), DriverError> {
        self.runtime.abort_observation().map_err(DriverError::from)
    }

    fn requires_restart(&self) -> bool {
        self.runtime.client.requires_restart()
    }
}

fn arguments(session: &DurableAgentSession) -> Result<Vec<String>, DriverLaunchError> {
    validate_profile(session).map_err(DriverLaunchError::safe)?;
    let mut arguments = Vec::new();
    if session.public.permission_mode == "workspace_write" {
        arguments.extend(["--permission-mode".to_owned(), "acceptEdits".to_owned()]);
    }
    arguments.extend([
        "agent".to_owned(),
        "--model".to_owned(),
        session.public.model.clone(),
        "--reasoning-effort".to_owned(),
        session.public.reasoning_effort.clone(),
        "stdio".to_owned(),
    ]);
    Ok(arguments)
}

async fn environment(
    state_root: &Path,
    session: &DurableAgentSession,
) -> Result<Vec<(String, String)>, DriverLaunchError> {
    let home = state_home(state_root, session).await?;
    let home = home
        .to_str()
        .ok_or_else(|| DriverLaunchError::safe(profile_error()))?;
    let mut result = vec![
        ("GROK_HOME".to_owned(), home.to_owned()),
        (
            "GROK_DEFAULT_SELECTED_PERMISSION".to_owned(),
            "reject".to_owned(),
        ),
    ];
    if let Some(auth) = auth_path() {
        let auth = auth
            .to_str()
            .ok_or_else(|| DriverLaunchError::safe(profile_error()))?;
        result.push(("GROK_AUTH_PATH".to_owned(), auth.to_owned()));
    }
    Ok(result)
}

async fn state_home(
    state_root: &Path,
    session: &DurableAgentSession,
) -> Result<PathBuf, DriverLaunchError> {
    let mut digest = Sha256::new();
    for value in [
        &session.public.room_id,
        &session.public.session_id,
        &session.runtime_profile_key,
    ] {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    }
    let home = state_root
        .join("provider-state")
        .join("grok")
        .join(format!("{:x}", digest.finalize()));
    tokio::fs::create_dir_all(&home)
        .await
        .map_err(|_| DriverLaunchError::safe(state_error()))?;
    secure_directory(&home).map_err(|_| DriverLaunchError::safe(state_error()))?;
    Ok(home)
}

fn secure_directory(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != rustix::process::geteuid().as_raw()
            || metadata.permissions().mode() & 0o077 != 0
        {
            return Err(std::io::Error::other("Grok state directory is not private"));
        }
    }
    #[cfg(not(unix))]
    {
        let metadata = std::fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(std::io::Error::other("Grok state directory is invalid"));
        }
    }
    Ok(())
}

fn auth_path() -> Option<PathBuf> {
    env::var_os("GROK_AUTH_PATH")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("GROK_HOME")
                .map(PathBuf::from)
                .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".grok")))
                .map(|home| home.join("auth.json"))
        })
}

fn validate_profile(session: &DurableAgentSession) -> Result<(), DriverError> {
    if valid_model_id(&session.public.model)
        && matches!(
            session.public.reasoning_effort.as_str(),
            "low" | "medium" | "high"
        )
        && session.public.service_tier.is_empty()
        && session.public.variant.is_empty()
        && matches!(
            session.public.permission_mode.as_str(),
            "meeting_read_only" | "workspace_write"
        )
    {
        Ok(())
    } else {
        Err(profile_error())
    }
}

const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The Grok ACP runtime profile is invalid.",
    )
}

const fn state_error() -> DriverError {
    DriverError::new(
        "provider_state_unavailable",
        "Private Grok provider state is unavailable.",
    )
}

#[cfg(test)]
#[path = "grok_acp_tests.rs"]
mod tests;
