use crate::{
    runtime_handle::{RuntimeHandlePlatform, parse_handle_id},
    runtime_lease::LeaseObservation,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObservationScope {
    ColdRestart,
    LiveSlot,
}

pub(crate) fn observation_proves_gone(
    handle_id: &str,
    durable_launch_token: &str,
    observation: &LeaseObservation,
    scope: ObservationScope,
) -> bool {
    if durable_launch_token.is_empty() {
        return false;
    }
    let Ok(handle) = parse_handle_id(handle_id) else {
        return false;
    };
    if !handle_matches_current_platform(handle.platform)
        || handle.launch_token != durable_launch_token
    {
        return false;
    }
    match observation {
        LeaseObservation::GenerationGone { launch_token } => launch_token == durable_launch_token,
        LeaseObservation::PreviousBoot {
            boot_identity,
            launch_token,
        } => {
            scope == ObservationScope::ColdRestart
                && launch_token == durable_launch_token
                && previous_boot_matches(&handle, boot_identity)
        }
        LeaseObservation::Missing => {
            scope == ObservationScope::ColdRestart && handle_is_from_previous_boot(&handle)
        }
        LeaseObservation::Active | LeaseObservation::Unknown => false,
    }
}

#[cfg(unix)]
const fn handle_matches_current_platform(platform: RuntimeHandlePlatform) -> bool {
    matches!(platform, RuntimeHandlePlatform::Unix)
}

#[cfg(windows)]
const fn handle_matches_current_platform(platform: RuntimeHandlePlatform) -> bool {
    matches!(platform, RuntimeHandlePlatform::Windows)
}

#[cfg(not(any(unix, windows)))]
const fn handle_matches_current_platform(_platform: RuntimeHandlePlatform) -> bool {
    false
}

#[cfg(unix)]
fn previous_boot_matches(
    handle: &crate::runtime_handle::RuntimeHandleIdentity,
    observed_boot: &str,
) -> bool {
    handle.boot_identity.as_deref() == Some(observed_boot)
        && crate::runtime_boot::current_identity().is_ok_and(|current| observed_boot != current)
}

#[cfg(not(unix))]
const fn previous_boot_matches(
    _handle: &crate::runtime_handle::RuntimeHandleIdentity,
    _observed_boot: &str,
) -> bool {
    false
}

#[cfg(unix)]
fn handle_is_from_previous_boot(handle: &crate::runtime_handle::RuntimeHandleIdentity) -> bool {
    handle
        .boot_identity
        .as_deref()
        .is_some_and(|boot_identity| {
            crate::runtime_boot::current_identity().is_ok_and(|current| boot_identity != current)
        })
}

#[cfg(not(unix))]
const fn handle_is_from_previous_boot(
    _handle: &crate::runtime_handle::RuntimeHandleIdentity,
) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::{ObservationScope, observation_proves_gone};
    use crate::runtime_lease::LeaseObservation;

    fn different_boot_identity(current: &str) -> String {
        let mut identity = current.to_owned();
        let replacement = if current.starts_with('0') { "1" } else { "0" };
        identity.replace_range(..1, replacement);
        identity
    }

    #[test]
    fn live_slot_never_accepts_a_previous_boot_marker() {
        let token = uuid::Uuid::new_v4().to_string();
        let current_boot = crate::runtime_boot::current_identity()
            .unwrap_or_else(|error| panic!("read current boot identity: {error}"));
        let handle = crate::runtime_handle::new_unix_handle_id(current_boot, &token);
        let previous_boot = different_boot_identity(current_boot);
        let observation = LeaseObservation::PreviousBoot {
            boot_identity: previous_boot,
            launch_token: token.clone(),
        };
        assert!(!observation_proves_gone(
            &handle,
            &token,
            &observation,
            ObservationScope::LiveSlot,
        ));
    }

    #[test]
    fn cold_proof_requires_one_strict_handle_marker_generation() {
        let token = uuid::Uuid::new_v4().to_string();
        let current_boot = crate::runtime_boot::current_identity()
            .unwrap_or_else(|error| panic!("read current boot identity: {error}"));
        let previous_boot = different_boot_identity(current_boot);
        let handle = crate::runtime_handle::new_unix_handle_id(&previous_boot, &token);
        let observation = LeaseObservation::PreviousBoot {
            boot_identity: previous_boot,
            launch_token: token.clone(),
        };
        assert!(observation_proves_gone(
            &handle,
            &token,
            &observation,
            ObservationScope::ColdRestart,
        ));
        assert!(!observation_proves_gone(
            &handle,
            &uuid::Uuid::new_v4().to_string(),
            &observation,
            ObservationScope::ColdRestart,
        ));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::{ObservationScope, observation_proves_gone};
    use crate::runtime_lease::LeaseObservation;

    #[test]
    fn windows_gone_requires_handle_durable_and_marker_token_match() {
        let token = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        let observation = LeaseObservation::GenerationGone {
            launch_token: token.clone(),
        };
        assert!(observation_proves_gone(
            &format!("runtime-v5-windows-{token}-{}", uuid::Uuid::new_v4()),
            &token,
            &observation,
            ObservationScope::ColdRestart,
        ));
        assert!(!observation_proves_gone(
            &format!("runtime-v5-windows-{other}-{}", uuid::Uuid::new_v4()),
            &token,
            &observation,
            ObservationScope::ColdRestart,
        ));
        assert!(!observation_proves_gone(
            "runtime-v5-windows-malformed",
            &token,
            &observation,
            ObservationScope::ColdRestart,
        ));
    }
}
