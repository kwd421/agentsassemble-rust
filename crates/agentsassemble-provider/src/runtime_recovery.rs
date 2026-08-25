use std::time::Duration;

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

use crate::{
    runtime::ProviderRuntimeObservation,
    runtime_absence::{ObservationScope, observation_proves_gone},
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
    classify_previous_runtime(session, &observation)
}

fn classify_previous_runtime(
    session: &DurableAgentSession,
    observation: &LeaseObservation,
) -> ProviderRuntimeObservation {
    if durable_runtime_identity_is_complete(session)
        && observation_proves_gone(
            &session.runtime_handle_id,
            &session.runtime_lease_token,
            observation,
            ObservationScope::ColdRestart,
        )
    {
        return ProviderRuntimeObservation::Gone;
    }
    if empty_pre_effect_authority(session)
        && matches!(
            observation,
            LeaseObservation::GenerationGone { .. }
                | LeaseObservation::PreviousBoot { .. }
                | LeaseObservation::Missing
        )
    {
        return ProviderRuntimeObservation::Gone;
    }
    match observation {
        LeaseObservation::Active => {
            if durable_runtime_identity_is_complete(session) {
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
        LeaseObservation::GenerationGone { .. } => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_lease_generation_mismatch".to_owned(),
        },
        LeaseObservation::PreviousBoot { .. } => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_boot_generation_mismatch".to_owned(),
        },
        LeaseObservation::Missing => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_lease_missing".to_owned(),
        },
        LeaseObservation::Unknown => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_lease_observation_failed".to_owned(),
        },
    }
}

fn durable_runtime_identity_is_complete(session: &DurableAgentSession) -> bool {
    !session.runtime_handle_id.is_empty()
        && !session.runtime_owner_id.is_empty()
        && !session.runtime_lease_token.is_empty()
}

fn empty_pre_effect_authority(session: &DurableAgentSession) -> bool {
    session.runtime_profile_version == CURRENT_RUNTIME_PROFILE_VERSION
        && session.runtime_handle_id.is_empty()
        && session.runtime_owner_id.is_empty()
        && session.runtime_lease_token.is_empty()
        && session.lifecycle_intent_action == "start"
        && session.lifecycle_intent_status == "prepared"
}

#[cfg(all(test, unix))]
mod tests {
    use super::{classify_previous_runtime, observe_previous_runtime};
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
        let launch_token = lease.token().to_owned();
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
        session.runtime_lease_token = launch_token.clone();
        assert!(matches!(
            observe_previous_runtime(&session).await,
            ProviderRuntimeObservation::Ambiguous { .. }
        ));

        let mut previous_handle = current_handle.into_bytes();
        let first_boot_digit = "runtime-v5-".len();
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

        session.runtime_lease_token = uuid::Uuid::new_v4().to_string();
        assert!(matches!(
            observe_previous_runtime(&session).await,
            ProviderRuntimeObservation::Ambiguous { .. }
        ));
    }

    #[test]
    fn previous_boot_marker_requires_the_same_durable_boot_and_launch_generation() {
        let lease = HeldRuntimeLease::prepare(
            "marker-boot-room",
            &format!("marker-boot-session-{}", uuid::Uuid::new_v4()),
        )
        .unwrap_or_else(|error| panic!("prepare marker-bound runtime lease: {error}"));
        let current_handle = lease.new_runtime_handle_id();
        let launch_token = lease.token().to_owned();
        lease.cleanup_pre_effect();
        let mut previous_handle = current_handle.clone().into_bytes();
        let first_boot_digit = "runtime-v5-".len();
        previous_handle[first_boot_digit] = if previous_handle[first_boot_digit] == b'0' {
            b'1'
        } else {
            b'0'
        };
        let previous_handle = String::from_utf8(previous_handle)
            .unwrap_or_else(|error| panic!("encode previous boot handle: {error}"));
        let previous_boot = crate::runtime_handle::parse_handle_id(&previous_handle)
            .unwrap_or_else(|error| panic!("parse previous boot handle: {error}"))
            .boot_identity
            .unwrap_or_else(|| panic!("Unix runtime handle must bind boot identity"));
        let mut session = durable_session(
            "marker-boot-room",
            "marker-boot-session",
            "Marker-bound Agent",
            "codex_live_session",
            "gpt-5.6-terra",
            "stdio_jsonl",
        );
        session.runtime_handle_id = previous_handle.clone();
        session.runtime_owner_id = "previous-supervisor".to_owned();
        session.runtime_lease_token = launch_token.clone();
        assert_eq!(
            classify_previous_runtime(
                &session,
                &crate::runtime_lease::LeaseObservation::PreviousBoot {
                    boot_identity: previous_boot.clone(),
                    launch_token: launch_token.clone(),
                },
            ),
            ProviderRuntimeObservation::Gone
        );

        session.runtime_handle_id = current_handle;
        assert!(matches!(
            classify_previous_runtime(
                &session,
                &crate::runtime_lease::LeaseObservation::PreviousBoot {
                    boot_identity: previous_boot.clone(),
                    launch_token: launch_token.clone(),
                },
            ),
            ProviderRuntimeObservation::Ambiguous { .. }
        ));

        session.runtime_handle_id = previous_handle;
        assert!(matches!(
            classify_previous_runtime(&session, &crate::runtime_lease::LeaseObservation::Unknown,),
            ProviderRuntimeObservation::Ambiguous { .. }
        ));
        assert!(matches!(
            classify_previous_runtime(
                &session,
                &crate::runtime_lease::LeaseObservation::GenerationGone {
                    launch_token: uuid::Uuid::new_v4().to_string(),
                },
            ),
            ProviderRuntimeObservation::Ambiguous { .. }
        ));

        session.runtime_handle_id.clear();
        session.runtime_owner_id.clear();
        session.runtime_lease_token.clear();
        session.lifecycle_intent_action = "start".to_owned();
        session.lifecycle_intent_status = "prepared".to_owned();
        assert_eq!(
            classify_previous_runtime(
                &session,
                &crate::runtime_lease::LeaseObservation::PreviousBoot {
                    boot_identity: previous_boot,
                    launch_token,
                },
            ),
            ProviderRuntimeObservation::Gone
        );
    }
}
