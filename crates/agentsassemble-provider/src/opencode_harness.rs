#[cfg(unix)]
use crate::runtime_lease::HeldRuntimeLease;
use crate::{
    driver::DriverError, launch_error::DriverLaunchError, opencode::OpenCodeDriver,
    provider_factory::ProductionDriverFactory, runtime::ProviderDriver,
};
use agentsassemble_domain::DurableAgentSession;

/// Stored value of the execution harness that runs an API agent inside OpenCode.
pub(crate) const NAME: &str = "opencode";

struct HarnessProvider {
    environment: &'static str,
    opencode_provider: &'static str,
}

const GEMINI: HarnessProvider = HarnessProvider {
    environment: "GOOGLE_GENERATIVE_AI_API_KEY",
    opencode_provider: "google",
};

// OpenCode documents this provider id and environment variable. They live in one
// place so the mapping can be corrected without touching selection or the driver.
const DEEPSEEK: HarnessProvider = HarnessProvider {
    environment: "DEEPSEEK_API_KEY",
    opencode_provider: "deepseek",
};

fn provider(provider_kind: &str) -> Option<&'static HarnessProvider> {
    match provider_kind {
        "gemini_api" => Some(&GEMINI),
        "deepseek_api" => Some(&DEEPSEEK),
        _ => None,
    }
}

/// Maps a stored API model onto the model id OpenCode expects.
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

/// The one credential pair added to the isolated OpenCode child environment.
pub(crate) fn environment(
    provider_kind: &str,
    secret: &str,
) -> Result<Vec<(String, String)>, DriverError> {
    let provider = provider(provider_kind).ok_or_else(profile_error)?;
    if secret.is_empty() || secret.contains('\0') {
        return Err(DriverError::new(
            "provider_credential_invalid",
            "The saved API credential cannot be passed to OpenCode.",
        ));
    }
    Ok(vec![(provider.environment.to_owned(), secret.to_owned())])
}

#[cfg(unix)]
pub(crate) async fn launch(
    factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    runtime_lease: &HeldRuntimeLease,
    credential: &crate::credentials::ProviderCredential,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let environment = environment(&session.public.provider_kind, credential.expose())?;
    let driver =
        OpenCodeDriver::spawn_harness(session, runtime_lease, factory.guardian()?, environment)
            .await?;
    Ok(Box::new(driver))
}

#[cfg(not(unix))]
pub(crate) async fn launch(
    _factory: &ProductionDriverFactory,
    session: &DurableAgentSession,
    credential: &crate::credentials::ProviderCredential,
) -> Result<Box<dyn ProviderDriver>, DriverLaunchError> {
    let environment = environment(&session.public.provider_kind, credential.expose())?;
    let driver = OpenCodeDriver::spawn_harness(session, environment).await?;
    Ok(Box::new(driver))
}

pub(crate) const fn profile_error() -> DriverError {
    DriverError::new(
        "invalid_runtime_profile",
        "The stored OpenCode harness profile is invalid.",
    )
}

#[cfg(test)]
#[path = "opencode_harness_tests.rs"]
mod tests;
