use agentsassemble_domain::DurableAgentSession;

use super::{
    DriverError, OwnedRuntime, ProviderAdapter, ProviderAdapterError, ProviderRuntimeGone,
    ProviderRuntimeObservation, RuntimeKey, RuntimeSlot, RuntimeState,
};
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
            if runtime.runtime_lease.cleanup_receipt_is_present() {
                let RuntimeState::Launching(mut runtime) =
                    std::mem::replace(&mut slot.state, RuntimeState::Vacant)
                else {
                    unreachable!("observed provider launch must remain owned");
                };
                runtime.runtime_lease.release_and_remove();
                return ProviderRuntimeObservation::Gone;
            }
            return match observe_runtime_lease(&session.public.room_id, &session.public.session_id)
            {
                LeaseObservation::Active => ProviderRuntimeObservation::LeaseUncertain {
                    handle_id: runtime.handle_id.clone(),
                    owner_id: runtime.owner_id.clone(),
                    reason_code: "provider_launch_cleanup_active".to_owned(),
                },
                LeaseObservation::Gone | LeaseObservation::Missing | LeaseObservation::Unknown => {
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
        if let Some(observation) = unavailable_running_health(runtime).await {
            return observation;
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

async fn unavailable_running_health(runtime: &OwnedRuntime) -> Option<ProviderRuntimeObservation> {
    let Ok(mut driver) = runtime.driver.try_lock() else {
        return Some(ProviderRuntimeObservation::LeaseUncertain {
            handle_id: runtime.handle_id.clone(),
            owner_id: runtime.owner_id.clone(),
            reason_code: "provider_turn_active".to_owned(),
        });
    };
    let reason_code = match driver.is_alive().await {
        Ok(true) => return None,
        Ok(false) => "provider_leader_exited",
        Err(_) => "runtime_health_unknown",
    };
    Some(ProviderRuntimeObservation::LeaseUncertain {
        handle_id: runtime.handle_id.clone(),
        owner_id: runtime.owner_id.clone(),
        reason_code: reason_code.to_owned(),
    })
}

pub(super) fn shutdown_launching_runtime(
    key: &RuntimeKey,
    slot: &mut RuntimeSlot,
) -> Option<Result<ProviderRuntimeGone, ProviderAdapterError>> {
    let RuntimeState::Launching(runtime) = &slot.state else {
        return None;
    };
    if !runtime.runtime_lease.cleanup_receipt_is_present() {
        return Some(Err(ProviderAdapterError::uncertain(
            DriverError::new(
                "provider_launch_cleanup_unconfirmed",
                "An interrupted provider launch could not be confirmed gone.",
            ),
            &runtime.handle_id,
            &runtime.owner_id,
        )));
    }
    let RuntimeState::Launching(runtime) = std::mem::replace(&mut slot.state, RuntimeState::Vacant)
    else {
        unreachable!("observed provider launch must remain owned");
    };
    let stopped = ProviderRuntimeGone {
        room_id: key.room_id.clone(),
        session_id: key.session_id.clone(),
        runtime_handle_id: runtime.handle_id.clone(),
        runtime_owner_id: runtime.owner_id.clone(),
        runtime_lease_token: runtime.runtime_lease.token().to_owned(),
    };
    slot.state = RuntimeState::StopConfirmed {
        handle_id: runtime.handle_id,
        owner_id: runtime.owner_id,
        runtime_lease: runtime.runtime_lease,
    };
    Some(Ok(stopped))
}
