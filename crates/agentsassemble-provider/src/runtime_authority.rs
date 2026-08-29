use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

use crate::{
    filesystem::{canonical_workspace, runtime_executable_identity},
    profile::runtime_profile_key,
    runtime::DriverError,
};

pub(crate) async fn revalidate_runtime_authority(
    session: &DurableAgentSession,
) -> Result<(), DriverError> {
    if session.runtime_profile_version != CURRENT_RUNTIME_PROFILE_VERSION {
        return Err(DriverError::new(
            "runtime_profile_unsupported",
            "The provider runtime profile version is unsupported.",
        ));
    }
    let expected_profile_key = runtime_profile_key([
        session.public.provider_kind.as_str(),
        session.public.runtime_kind.as_str(),
        session.executable.as_str(),
        session.executable_identity.as_str(),
        session.workspace.as_str(),
        session.workspace_identity.as_str(),
        session.public.model.as_str(),
        session.public.reasoning_effort.as_str(),
        session.public.service_tier.as_str(),
        session.public.variant.as_str(),
        session.public.execution_harness.as_str(),
        session.public.permission_mode.as_str(),
        session.public.persona_card_id.as_ref(),
        session.public.transport.as_str(),
    ]);
    if expected_profile_key != session.runtime_profile_key {
        return Err(DriverError::new(
            "runtime_profile_changed",
            "The provider runtime profile no longer matches its durable identity.",
        ));
    }
    let workspace = canonical_workspace(session.workspace.clone())
        .await
        .map_err(|_| {
            DriverError::new(
                "workspace_authority_changed",
                "The provider workspace authority could not be revalidated.",
            )
        })?;
    if workspace.0 != session.workspace || workspace.1 != session.workspace_identity {
        return Err(DriverError::new(
            "workspace_authority_changed",
            "The provider workspace authority changed after selection.",
        ));
    }
    let executable =
        runtime_executable_identity(&session.public.provider_kind, session.executable.clone())
            .await
            .map_err(|_| {
                DriverError::new(
                    "executable_authority_changed",
                    "The provider executable authority could not be revalidated.",
                )
            })?;
    if executable != session.executable_identity {
        return Err(DriverError::new(
            "executable_authority_changed",
            "The provider executable authority changed after selection.",
        ));
    }
    Ok(())
}
