use agentsassemble_domain::DurableAgentSession;

use super::{
    DriverError, OwnedRuntime, ProviderAdapterError, ProviderRuntimeStarted,
    ProviderSessionAttachment, revalidate_runtime_authority,
};

fn started(
    session: &DurableAgentSession,
    runtime: &OwnedRuntime,
    runtime_reused: bool,
    attachment: ProviderSessionAttachment,
) -> Result<ProviderRuntimeStarted, DriverError> {
    if attachment
        .observed_model_id
        .as_deref()
        .is_some_and(|model| model != session.public.model)
    {
        return Err(DriverError::new(
            "provider_model_mismatch",
            "The provider attached a session for a different model.",
        ));
    }
    Ok(ProviderRuntimeStarted {
        runtime_handle_id: runtime.handle_id.clone(),
        runtime_owner_id: runtime.owner_id.clone(),
        provider_session_id: attachment.provider_session_id,
        runtime_reused,
        provider_session_reused: attachment.reused,
        provider_session_active: true,
    })
}

pub(super) async fn reuse_owned_runtime(
    session: &DurableAgentSession,
    runtime: &mut OwnedRuntime,
) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
    validate_owned_runtime(session, runtime)?;
    let mut driver = runtime.driver.lock().await;
    match driver.is_alive() {
        Ok(true) => {}
        Ok(false) => {
            return Err(ProviderAdapterError::uncertain(
                DriverError::new(
                    "provider_runtime_exited",
                    "The owned provider runtime exited before it became ready.",
                ),
                &runtime.handle_id,
                &runtime.owner_id,
            ));
        }
        Err(error) => {
            return Err(ProviderAdapterError::uncertain(
                error,
                &runtime.handle_id,
                &runtime.owner_id,
            ));
        }
    }
    if let Err(error) = revalidate_runtime_authority(session).await {
        return Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        ));
    }
    match driver.attach_session(session).await {
        Ok(attachment) => started(session, runtime, true, attachment).map_err(|error| {
            ProviderAdapterError::uncertain(error, &runtime.handle_id, &runtime.owner_id)
        }),
        Err(error) => Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        )),
    }
}

pub(super) async fn initialize_owned_runtime(
    session: &DurableAgentSession,
    runtime: &mut OwnedRuntime,
) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
    let mut driver = runtime.driver.lock().await;
    match driver.attach_session(session).await {
        Ok(attachment) => started(session, runtime, false, attachment).map_err(|error| {
            ProviderAdapterError::uncertain(error, &runtime.handle_id, &runtime.owner_id)
        }),
        Err(error) => Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        )),
    }
}

pub(super) fn validate_owned_runtime(
    session: &DurableAgentSession,
    runtime: &OwnedRuntime,
) -> Result<(), ProviderAdapterError> {
    let durable_handle_matches =
        session.runtime_handle_id.is_empty() || session.runtime_handle_id == runtime.handle_id;
    let durable_owner_matches =
        session.runtime_owner_id.is_empty() || session.runtime_owner_id == runtime.owner_id;
    if runtime.profile_key != session.runtime_profile_key
        || !durable_handle_matches
        || !durable_owner_matches
    {
        return Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "runtime_owner_mismatch",
                "The provider runtime does not match the durable session authority.",
            ),
            &runtime.handle_id,
            &runtime.owner_id,
        ));
    }
    Ok(())
}
