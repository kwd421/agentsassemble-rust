use agentsassemble_domain::DurableAgentSession;

use super::{ProviderAdapter, ProviderRuntimeObservation, RuntimeState};
use crate::{
    runtime_authority::revalidate_runtime_authority,
    runtime_lease::{LeaseObservation, observe_runtime_lease},
    runtime_recovery::observe_previous_runtime,
};

impl ProviderAdapter {
    pub async fn observe(&self, session: &DurableAgentSession) -> ProviderRuntimeObservation {
        let Some(slot) = self
            .existing_slot(&session.public.room_id, &session.public.session_id)
            .await
        else {
            return observe_previous_runtime(session).await;
        };
        let mut slot = slot.lock().await;
        if let RuntimeState::Launching(runtime) = &slot.state {
            if runtime.handle_id != session.runtime_handle_id
                || runtime.owner_id != session.runtime_owner_id
            {
                return ProviderRuntimeObservation::Ambiguous {
                    reason_code: "runtime_identity_mismatch".to_owned(),
                };
            }
            return match observe_runtime_lease(&session.public.room_id, &session.public.session_id)
            {
                LeaseObservation::Gone => {
                    let RuntimeState::Launching(mut runtime) =
                        std::mem::replace(&mut slot.state, RuntimeState::Vacant)
                    else {
                        unreachable!("observed provider launch must remain owned");
                    };
                    runtime.runtime_lease.release_and_remove();
                    ProviderRuntimeObservation::Gone
                }
                LeaseObservation::Active => ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "provider_launch_cleanup_active".to_owned(),
                },
                LeaseObservation::Missing | LeaseObservation::Unknown => {
                    ProviderRuntimeObservation::LeaseUncertain {
                        handle_id: runtime.handle_id.clone(),
                        owner_id: runtime.owner_id.clone(),
                        reason_code: "provider_launch_cleanup_unconfirmed".to_owned(),
                    }
                }
            };
        }
        if let RuntimeState::StopConfirmed {
            handle_id,
            owner_id,
            ..
        } = &slot.state
        {
            return if handle_id == &session.runtime_handle_id
                && owner_id == &session.runtime_owner_id
            {
                ProviderRuntimeObservation::Gone
            } else {
                ProviderRuntimeObservation::Ambiguous {
                    reason_code: "runtime_identity_mismatch".to_owned(),
                }
            };
        }
        let RuntimeState::Running(runtime) = &mut slot.state else {
            return ProviderRuntimeObservation::Gone;
        };
        if runtime.handle_id != session.runtime_handle_id
            || runtime.profile_key != session.runtime_profile_key
        {
            return ProviderRuntimeObservation::Ambiguous {
                reason_code: "runtime_identity_mismatch".to_owned(),
            };
        }
        match runtime.driver.is_alive() {
            Ok(true) => {}
            Ok(false) => {
                return ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "provider_leader_exited".to_owned(),
                };
            }
            Err(_) => {
                return ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "runtime_health_unknown".to_owned(),
                };
            }
        }
        if let Err(error) = revalidate_runtime_authority(session).await {
            return ProviderRuntimeObservation::LeaseUncertain {
                handle_id: runtime.handle_id.clone(),
                owner_id: runtime.owner_id.clone(),
                reason_code: error.code.to_owned(),
            };
        }
        ProviderRuntimeObservation::Adopted {
            handle_id: runtime.handle_id.clone(),
            previous_owner_id: session.runtime_owner_id.clone(),
            new_owner_id: runtime.owner_id.clone(),
            runtime_profile_key: runtime.profile_key.clone(),
        }
    }
}
