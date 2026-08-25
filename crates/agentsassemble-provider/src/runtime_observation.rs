use agentsassemble_domain::DurableAgentSession;

use super::{
    DriverError, OwnedRuntime, ProviderAdapter, ProviderAdapterError, ProviderRuntimeGone,
    ProviderRuntimeObservation, RuntimeKey, RuntimeSlot, RuntimeState,
};
use crate::{
    runtime_absence::{ObservationScope, observation_proves_gone},
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
        if let Some(observation) = observe_launching_runtime(&mut slot, session) {
            return observation;
        }
        if let RuntimeState::StopConfirmed {
            handle_id,
            owner_id,
            runtime_lease,
        } = &slot.state
        {
            return if handle_id == &session.runtime_handle_id
                && owner_id == &session.runtime_owner_id
                && runtime_lease.token() == session.runtime_lease_token
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
            || runtime.lease_token != session.runtime_lease_token
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

fn observe_launching_runtime(
    slot: &mut RuntimeSlot,
    session: &DurableAgentSession,
) -> Option<ProviderRuntimeObservation> {
    let RuntimeState::Launching(runtime) = &slot.state else {
        return None;
    };
    if runtime.handle_id != session.runtime_handle_id
        || runtime.owner_id != session.runtime_owner_id
        || runtime.runtime_lease.token() != session.runtime_lease_token
    {
        return Some(ProviderRuntimeObservation::Ambiguous {
            reason_code: "runtime_identity_mismatch".to_owned(),
        });
    }
    if !runtime.effect_started {
        return Some(ProviderRuntimeObservation::Gone);
    }
    if runtime.runtime_lease.cleanup_receipt_is_present() {
        return Some(ProviderRuntimeObservation::Gone);
    }
    Some(
        match observe_runtime_lease(&session.public.room_id, &session.public.session_id) {
            LeaseObservation::Active => ProviderRuntimeObservation::LeaseUncertain {
                handle_id: runtime.handle_id.clone(),
                owner_id: runtime.owner_id.clone(),
                reason_code: "provider_launch_cleanup_active".to_owned(),
            },
            observation
                if observation_proves_gone(
                    &runtime.handle_id,
                    runtime.runtime_lease.token(),
                    &observation,
                    ObservationScope::LiveSlot,
                ) =>
            {
                ProviderRuntimeObservation::Gone
            }
            LeaseObservation::GenerationGone { .. }
            | LeaseObservation::PreviousBoot { .. }
            | LeaseObservation::Missing
            | LeaseObservation::Unknown => ProviderRuntimeObservation::LeaseUncertain {
                handle_id: runtime.handle_id.clone(),
                owner_id: runtime.owner_id.clone(),
                reason_code: "provider_launch_cleanup_unconfirmed".to_owned(),
            },
        },
    )
}

async fn unavailable_running_health(runtime: &OwnedRuntime) -> Option<ProviderRuntimeObservation> {
    let Ok(mut driver) = runtime.driver.try_take() else {
        return Some(ProviderRuntimeObservation::LeaseUncertain {
            handle_id: runtime.handle_id.clone(),
            owner_id: runtime.owner_id.clone(),
            reason_code: "provider_turn_active".to_owned(),
        });
    };
    let reason_code = match driver.is_alive().await {
        Ok(true) if driver.attachment_replay_is_safe() => None,
        Ok(true) => Some("provider_session_creation_unconfirmed"),
        Ok(false) => Some("provider_leader_exited"),
        Err(_) => Some("runtime_health_unknown"),
    };
    runtime.driver.put(driver).await;
    reason_code.map(|reason_code| ProviderRuntimeObservation::LeaseUncertain {
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
    if !runtime.effect_started {
        let stopped = ProviderRuntimeGone {
            room_id: key.room_id.clone(),
            session_id: key.session_id.clone(),
            runtime_handle_id: runtime.handle_id.clone(),
            runtime_owner_id: runtime.owner_id.clone(),
            runtime_lease_token: runtime.runtime_lease.token().to_owned(),
        };
        let RuntimeState::Launching(runtime) =
            std::mem::replace(&mut slot.state, RuntimeState::Vacant)
        else {
            unreachable!("pre-effect provider reservation must remain owned");
        };
        slot.state = RuntimeState::StopConfirmed {
            handle_id: runtime.handle_id,
            owner_id: runtime.owner_id,
            runtime_lease: runtime.runtime_lease,
        };
        return Some(Ok(stopped));
    }
    let observation = observe_runtime_lease(&key.room_id, &key.session_id);
    if !runtime.runtime_lease.cleanup_receipt_is_present()
        && !observation_proves_gone(
            &runtime.handle_id,
            runtime.runtime_lease.token(),
            &observation,
            ObservationScope::LiveSlot,
        )
    {
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
