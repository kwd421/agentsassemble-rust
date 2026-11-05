use agentsassemble_domain::{
    CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession, durable_runtime_profile_key,
};

use crate::{
    filesystem::{canonical_workspace, runtime_executable_identity},
    registration::{provider_registration_by_id, provider_registration_by_profile},
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
    if durable_runtime_profile_key(session) != session.runtime_profile_key {
        return Err(DriverError::new(
            "runtime_profile_changed",
            "The provider runtime profile no longer matches its durable identity.",
        ));
    }
    let registration = provider_registration_by_profile(
        &session.public.provider_kind,
        &session.public.runtime_kind,
        &session.public.transport,
    )
    .ok_or_else(|| {
        DriverError::new(
            "invalid_runtime_profile",
            "The stored provider runtime profile is unsupported.",
        )
    })?;
    if registration.workspace_required || !session.workspace.is_empty() {
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
    } else if !session.workspace_identity.is_empty() {
        return Err(DriverError::new(
            "workspace_authority_changed",
            "The provider workspace authority is inconsistent.",
        ));
    }
    let executable_registration =
        if session.public.execution_harness == crate::opencode_harness::NAME {
            provider_registration_by_id("opencode").unwrap_or(registration)
        } else {
            registration
        };
    if executable_registration.executable_required {
        let executable = runtime_executable_identity(
            executable_registration.provider_kind,
            session.executable.clone(),
        )
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
    } else if !session.executable.is_empty() || !session.executable_identity.is_empty() {
        return Err(DriverError::new(
            "executable_authority_changed",
            "The provider executable authority is inconsistent.",
        ));
    }
    Ok(())
}
