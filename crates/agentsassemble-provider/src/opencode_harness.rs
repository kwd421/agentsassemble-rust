use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

#[cfg(unix)]
use crate::runtime_lease::HeldRuntimeLease;
use crate::{
    driver::DriverError,
    launch_error::DriverLaunchError,
    opencode::OpenCodeDriver,
    opencode_key_broker::{KeyBroker, Upstream},
    provider_factory::ProductionDriverFactory,
    runtime::ProviderDriver,
};
use agentsassemble_domain::DurableAgentSession;

/// Stored value of the execution harness that runs an API agent inside `OpenCode`.
pub(crate) const NAME: &str = "opencode";

struct HarnessProvider {
    opencode_provider: &'static str,
}

const GEMINI: HarnessProvider = HarnessProvider {
    opencode_provider: "google",
};

const DEEPSEEK: HarnessProvider = HarnessProvider {
    opencode_provider: "deepseek",
};

fn provider(provider_kind: &str) -> Option<&'static HarnessProvider> {
    match provider_kind {
        "gemini_api" => Some(&GEMINI),
        "deepseek_api" => Some(&DEEPSEEK),
        _ => None,
    }
}

/// Maps a stored API model onto the model id `OpenCode` expects.
pub(crate) fn driver_model(provider_kind: &str, model: &str) -> Option<String> {
    let provider = provider(provider_kind)?;
    if model.is_empty()
        || model.len() > 128
        || model.contains('/')
        || !model
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(&byte))
    {
        return None;
    }
    Some(format!("{}/{model}", provider.opencode_provider))
}

/// Starts the loopback key broker that holds this session's API key. `OpenCode`
/// only ever receives the broker's address and per-launch token.
pub(crate) async fn key_broker(
    session: &DurableAgentSession,
    secret: &str,
) -> Result<KeyBroker, DriverError> {
    let upstream =
        Upstream::for_provider(&session.public.provider_kind).ok_or_else(profile_error)?;
    driver_model(&session.public.provider_kind, &session.public.model).ok_or_else(profile_error)?;
    KeyBroker::start(upstream, session.public.model.clone(), secret.to_owned()).await
}

/// Folder an API agent without an owner-selected workspace runs in. `OpenCode`
/// keeps each session, and the directory it was opened in, in the owner's data
/// root, so the folder must outlive one launch for the session to resume.
pub(crate) async fn conversation_workspace(
    state_root: Option<&Path>,
    session: &DurableAgentSession,
) -> Result<Option<PathBuf>, DriverLaunchError> {
    if !session.workspace.is_empty() {
        return Ok(None);
    }
    let state_root = state_root.ok_or_else(|| DriverLaunchError::safe(profile_error()))?;
    let mut digest = Sha256::new();
    for value in [&session.public.room_id, &session.public.session_id] {
        digest.update(value.len().to_le_bytes());
        digest.update(value.as_bytes());
    }
    let folder = state_root
        .join("provider-state")
        .join("opencode-conversation")
        .join(format!("{:x}", digest.finalize()));
    tokio::fs::create_dir_all(&folder)
        .await
        .map_err(|_| DriverLaunchError::safe(workspace_error()))?;
    crate::filesystem::secure_private_directory(&folder)
        .map_err(|_| DriverLaunchError::safe(workspace_error()))?;
    let folder = tokio::fs::canonicalize(&folder)
        .await
        .map_err(|_| DriverLaunchError::safe(workspace_error()))?;
    Ok(Some(folder))
}

#[cfg(unix)]
pub(crate) async fn launch(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    runtime_lease: &HeldRuntimeLease,
    credential: &crate::credentials::ProviderCredential,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let workspace = conversation_workspace(factory.state_root.as_deref(), session).await?;
    let broker = key_broker(session, credential.expose()).await?;
    let driver = OpenCodeDriver::spawn_harness(
        session,
        runtime_lease,
        factory.guardian()?,
        broker,
        workspace,
    )
    .await?;
    Ok(Box::new(driver))
}

#[cfg(not(unix))]
pub(crate) async fn launch(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    credential: &crate::credentials::ProviderCredential,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let workspace = conversation_workspace(factory.state_root.as_deref(), session).await?;
    let broker = key_broker(session, credential.expose()).await?;
    let driver = OpenCodeDriver::spawn_harness(session, broker, workspace).await?;
    Ok(Box::new(driver))
}

pub(crate) const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The stored OpenCode harness profile is invalid.",
    )
}

const fn workspace_error() -> DriverError {
    DriverError::new(
        "provider_launch_failed",
        "The OpenCode conversation folder could not be prepared.",
    )
}

#[cfg(test)]
#[path = "opencode_harness_tests.rs"]
mod tests;
