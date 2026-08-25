use super::{DriverError, ProviderAdapterError, RuntimeSlot, RuntimeState};

pub(super) fn begin_launch_effect(slot: &mut RuntimeSlot) -> Result<(), ProviderAdapterError> {
    let launch_marker = match &slot.state {
        RuntimeState::Launching(runtime) => runtime.runtime_lease.begin_launch_effect(),
        _ => unreachable!("new provider runtime slot must be launching"),
    };
    launch_marker.map_err(|_| {
        ProviderAdapterError::safe(DriverError::new(
            "provider_custody_unavailable",
            "The provider launch authority could not be established.",
        ))
    })
}
