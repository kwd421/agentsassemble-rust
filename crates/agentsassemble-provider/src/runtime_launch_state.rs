use super::{DriverError, ProviderAdapterError, RuntimeSlot, RuntimeState};

pub(super) fn begin_launch_effect(slot: &mut RuntimeSlot) -> Result<(), ProviderAdapterError> {
    let launch_marker = match &slot.state {
        RuntimeState::Launching(runtime) => runtime.runtime_lease.begin_launch_effect(),
        _ => unreachable!("new provider runtime slot must be launching"),
    };
    if launch_marker.is_ok() {
        return Ok(());
    }
    let RuntimeState::Launching(runtime) = std::mem::replace(&mut slot.state, RuntimeState::Vacant)
    else {
        unreachable!("failed launch marker must remain owned");
    };
    runtime.runtime_lease.cleanup_pre_effect();
    Err(ProviderAdapterError::safe(DriverError::new(
        "provider_custody_unavailable",
        "The provider launch authority could not be established.",
    )))
}
