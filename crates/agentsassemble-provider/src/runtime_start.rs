use agentsassemble_domain::DurableAgentSession;
use tokio_util::sync::CancellationToken;

use super::{
    DriverError, OwnedRuntime, ProviderAdapterError, ProviderDriver, ProviderRuntimeStarted,
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
        runtime_lease_token: runtime.lease_token.clone(),
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
    if let Err(error) = revalidate_runtime_authority(session).await {
        return Err(ProviderAdapterError::uncertain(
            error,
            &runtime.handle_id,
            &runtime.owner_id,
        ));
    }
    let attachment = attach_owned(
        runtime.driver.clone(),
        runtime.turn_cancellation.clone(),
        session.clone(),
        true,
        &runtime.handle_id,
        &runtime.owner_id,
    )
    .await?;
    started(session, runtime, true, attachment).map_err(|error| {
        ProviderAdapterError::uncertain(error, &runtime.handle_id, &runtime.owner_id)
    })
}

pub(super) async fn initialize_owned_runtime(
    session: &DurableAgentSession,
    runtime: &mut OwnedRuntime,
) -> Result<ProviderRuntimeStarted, ProviderAdapterError> {
    let attachment = attach_owned(
        runtime.driver.clone(),
        runtime.turn_cancellation.clone(),
        session.clone(),
        false,
        &runtime.handle_id,
        &runtime.owner_id,
    )
    .await?;
    started(session, runtime, false, attachment).map_err(|error| {
        ProviderAdapterError::uncertain(error, &runtime.handle_id, &runtime.owner_id)
    })
}

async fn attach_owned(
    driver_cell: std::sync::Arc<super::runtime_driver::DriverCell>,
    cancellation: CancellationToken,
    session: DurableAgentSession,
    require_health: bool,
    handle_id: &str,
    owner_id: &str,
) -> Result<ProviderSessionAttachment, ProviderAdapterError> {
    let task = tokio::spawn(async move {
        let mut driver = tokio::select! {
            biased;
            () = cancellation.cancelled() => return Err(attachment_cancelled()),
            driver = driver_cell.take() => driver?,
        };
        let result = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(attachment_cancelled()),
            result = attach_driver_session(driver.as_mut(), &session, require_health) => result,
        };
        driver_cell.put(driver).await;
        result
    });
    task.await
        .map_err(|_| {
            ProviderAdapterError::uncertain(
                DriverError::new(
                    "provider_driver_owner_failed",
                    "The provider driver owner ended without returning attachment authority.",
                ),
                handle_id,
                owner_id,
            )
        })?
        .map_err(|error| ProviderAdapterError::uncertain(error, handle_id, owner_id))
}

async fn attach_driver_session(
    driver: &mut dyn ProviderDriver,
    session: &DurableAgentSession,
    require_health: bool,
) -> Result<ProviderSessionAttachment, DriverError> {
    if driver.requires_restart() {
        return Err(DriverError::new(
            "provider_runtime_restart_required",
            "The owned provider runtime must be stopped before it can be reused.",
        ));
    }
    if require_health {
        match driver.is_alive().await {
            Ok(true) => {}
            Ok(false) => {
                return Err(DriverError::new(
                    "provider_runtime_exited",
                    "The owned provider runtime exited before it became ready.",
                ));
            }
            Err(error) => return Err(error),
        }
    }
    driver.attach_session(session).await
}

const fn attachment_cancelled() -> DriverError {
    DriverError::new(
        "provider_attachment_cancelled",
        "The provider attachment was cancelled for owned runtime shutdown.",
    )
}

pub(super) fn validate_owned_runtime(
    session: &DurableAgentSession,
    runtime: &OwnedRuntime,
) -> Result<(), ProviderAdapterError> {
    let durable_handle_matches =
        session.runtime_handle_id.is_empty() || session.runtime_handle_id == runtime.handle_id;
    let durable_owner_matches =
        session.runtime_owner_id.is_empty() || session.runtime_owner_id == runtime.owner_id;
    let durable_lease_matches = session.runtime_lease_token.is_empty()
        || session.runtime_lease_token == runtime.lease_token;
    if runtime.profile_key != session.runtime_profile_key
        || !durable_handle_matches
        || !durable_owner_matches
        || !durable_lease_matches
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
