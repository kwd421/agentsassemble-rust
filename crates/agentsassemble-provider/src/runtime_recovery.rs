use std::time::Duration;

use agentsassemble_domain::{CURRENT_RUNTIME_PROFILE_VERSION, DurableAgentSession};

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
        LeaseObservation::Unknown
            if session.runtime_profile_version == CURRENT_RUNTIME_PROFILE_VERSION
                && session.runtime_handle_id.is_empty()
                && session.runtime_owner_id.is_empty() =>
        {
            ProviderRuntimeObservation::Gone
        }
        LeaseObservation::Unknown => ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_not_owned".to_owned(),
        },
    }
}
