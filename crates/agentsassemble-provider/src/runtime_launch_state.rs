use super::{DriverError, LaunchingRuntime, ProviderAdapterError, RuntimeSlot, RuntimeState};

pub(super) fn begin_launch_effect(
    slot: &mut RuntimeSlot,
) -> Result<&mut LaunchingRuntime, ProviderAdapterError> {
    let RuntimeState::Launching(runtime) = &mut slot.state else {
        unreachable!("new provider runtime slot must be launching");
    };
    runtime.runtime_lease.begin_launch_effect().map_err(|_| {
        ProviderAdapterError::safe(DriverError::new(
            "provider_custody_unavailable",
            "The provider launch authority could not be established.",
        ))
    })?;
    runtime.effect_started = true;
    Ok(runtime)
}
