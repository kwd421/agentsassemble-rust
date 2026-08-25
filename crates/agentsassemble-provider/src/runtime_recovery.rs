use std::time::Duration;

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

#[cfg(unix)]
use crate::runtime_boot::handle_is_from_current_boot;
use crate::{
    runtime::ProviderRuntimeObservation,
    runtime_lease::{LeaseObservation, observe_runtime_lease},
};

pub(crate) async fn observe_previous_runtime(
    session: &DurableAgentSession,
) -> ProviderRuntimeObservation {
    let mut observation =
        observe_runtime_lease(&session.public.room_id, &session.public.session_id);
    if observation == LeaseObservation::Active {
        for _ in 0..100 {
            tokio::time::sleep(Duration::from_millis(10)).await;
            observation =
                observe_runtime_lease(&session.public.room_id, &session.public.session_id);
            if observation != LeaseObservation::Active {
                break;
            }
        }
    }
    match observation {
        LeaseObservation::Active => {
            if !session.runtime_handle_id.is_empty() && !session.runtime_owner_id.is_empty() {
                return ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: session.runtime_handle_id.clone(),
                    owner_id: session.runtime_owner_id.clone(),
                    reason_code: "previous_runtime_guardian_active".to_owned(),
                };
            }
            ProviderRuntimeObservation::Ambiguous {
                reason_code: "previous_runtime_guardian_active".to_owned(),
            }
        }
        LeaseObservation::Gone => ProviderRuntimeObservation::Gone,
        LeaseObservation::Missing | LeaseObservation::Unknown
            if previous_boot_proves_absence(&session.runtime_handle_id) =>
        {
            ProviderRuntimeObservation::Gone
        }
        LeaseObservation::Missing
            if session.runtime_profile_version == CURRENT_RUNTIME_PROFILE_VERSION
                && session.runtime_handle_id.is_empty()
                && session.runtime_owner_id.is_empty()
                && session.lifecycle_intent_action == "start"
                && matches!(
                    session.lifecycle_intent_status.as_str(),
                    "prepared" | "effect_inflight" | "unconfirmed"
                ) =>
        {
            ProviderRuntimeObservation::Gone
        }
        LeaseObservation::Missing => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_lease_missing".to_owned(),
        },
        LeaseObservation::Unknown => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_lease_observation_failed".to_owned(),
        },
    }
}

#[cfg(unix)]
fn previous_boot_proves_absence(handle_id: &str) -> bool {
    !handle_id.is_empty() && matches!(handle_is_from_current_boot(handle_id), Ok(false))
}

#[cfg(not(unix))]
const fn previous_boot_proves_absence(_handle_id: &str) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::observe_previous_runtime;
    use crate::{
        runtime::ProviderRuntimeObservation, runtime_lease::HeldRuntimeLease,
        test_support::durable_session,
    };

    #[tokio::test]
    async fn missing_lease_is_gone_only_across_a_proven_boot_boundary() {
        let suffix = uuid::Uuid::new_v4();
        let room_id = format!("missing-boot-room-{suffix}");
        let session_id = format!("missing-boot-session-{suffix}");
        let lease = HeldRuntimeLease::prepare(&room_id, &session_id)
            .unwrap_or_else(|error| panic!("prepare boot-bound runtime lease: {error}"));
        let current_handle = lease.new_runtime_handle_id();
        lease.cleanup_pre_effect();
        let mut session = durable_session(
            &room_id,
            &session_id,
            "Boot-bound Agent",
            "codex_live_session",
            "gpt-5.6-terra",
            "stdio_jsonl",
        );
        session.runtime_handle_id = current_handle.clone();
        session.runtime_owner_id = "previous-supervisor".to_owned();
        assert!(matches!(
            observe_previous_runtime(&session).await,
            ProviderRuntimeObservation::Ambiguous { .. }
        ));

        let mut previous_handle = current_handle.into_bytes();
        let first_boot_digit = "runtime-v4-".len();
        previous_handle[first_boot_digit] = if previous_handle[first_boot_digit] == b'0' {
            b'1'
        } else {
            b'0'
        };
        session.runtime_handle_id = String::from_utf8(previous_handle)
            .unwrap_or_else(|error| panic!("encode prior boot handle: {error}"));
        assert_eq!(
            observe_previous_runtime(&session).await,
            ProviderRuntimeObservation::Gone
        );
    }
}
