//! Retained process-failure delivery from live runtime slots to their room owner.
use agentsassemble_domain::DurableAgentSession;
use futures_util::{StreamExt, stream::FuturesUnordered};

use super::{ProviderAdapter, RuntimeState};

/// An exact owned-generation failure notification. It never proves process absence.
pub struct ProviderRuntimeFailure {
    room_id: String,
    session_id: String,
    handle_id: String,
    owner_id: String,
    lease_token: String,
}

impl ProviderRuntimeFailure {
    #[must_use]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    #[must_use]
    pub fn matches(&self, session: &DurableAgentSession) -> bool {
        self.room_id == session.public.room_id
            && self.session_id == session.public.session_id
            && self.handle_id == session.runtime_handle_id
            && self.owner_id == session.runtime_owner_id
            && self.lease_token == session.runtime_lease_token
    }
}

impl ProviderAdapter {
    /// Waits on current room runtimes without polling or consuming the notification.
    /// The room actor rebuilds this wait after its serialized lifecycle commands,
    /// which are the only managed launch entry. Cancellation loses no notification.
    pub async fn wait_for_runtime_failure(&self, room_id: &str) -> ProviderRuntimeFailure {
        let slots: Vec<_> = self
            .owner
            .runtimes
            .lock()
            .await
            .iter()
            .filter(|(key, _)| key.room_id == room_id)
            .map(|(key, slot)| (key.clone(), slot.clone()))
            .collect();
        let mut pending = FuturesUnordered::new();
        for (key, slot) in slots {
            let slot = slot.lock().await;
            let RuntimeState::Running(runtime) = &slot.state else {
                continue;
            };
            let Some(signal) = runtime.failure_signal.clone() else {
                continue;
            };
            let failure = ProviderRuntimeFailure {
                room_id: key.room_id,
                session_id: key.session_id,
                handle_id: runtime.handle_id.clone(),
                owner_id: runtime.owner_id.clone(),
                lease_token: runtime.lease_token.clone(),
            };
            pending.push(async move {
                signal.cancelled().await;
                failure
            });
        }
        match pending.next().await {
            Some(failure) => failure,
            None => std::future::pending().await,
        }
    }

    /// Completes notification delivery after its room owner commits or retires it.
    pub async fn acknowledge_runtime_failure(&self, failure: &ProviderRuntimeFailure) {
        let Some(slot) = self
            .existing_slot(&failure.room_id, &failure.session_id)
            .await
        else {
            return;
        };
        let mut slot = slot.lock().await;
        if let RuntimeState::Running(runtime) = &mut slot.state
            && runtime.handle_id == failure.handle_id
            && runtime.owner_id == failure.owner_id
            && runtime.lease_token == failure.lease_token
        {
            runtime.failure_signal = None;
        }
    }
}
