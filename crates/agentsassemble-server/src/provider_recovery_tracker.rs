use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
};

use agentsassemble_persistence::AgentTurnAssignment;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ProviderRecoveryKey {
    room_id: String,
    session_id: String,
    turn_id: String,
    turn_generation: u64,
    execution_id: String,
    runtime_handle_id: String,
    runtime_owner_id: String,
    runtime_lease_token: String,
}

#[derive(Clone, Default)]
pub(crate) struct ProviderRecoveryTracker {
    queued: Arc<Mutex<HashSet<ProviderRecoveryKey>>>,
}

pub(crate) struct ProviderRecoveryGuard {
    tracker: ProviderRecoveryTracker,
    key: Option<ProviderRecoveryKey>,
}

impl ProviderRecoveryTracker {
    pub(crate) fn try_claim(
        &self,
        assignment: &AgentTurnAssignment,
    ) -> Option<ProviderRecoveryGuard> {
        let session = &assignment.session;
        let key = ProviderRecoveryKey {
            room_id: session.public.room_id.clone(),
            session_id: session.public.session_id.clone(),
            turn_id: assignment.turn_id.clone(),
            turn_generation: assignment.turn_generation,
            execution_id: assignment.execution_id.clone(),
            runtime_handle_id: session.runtime_handle_id.clone(),
            runtime_owner_id: session.runtime_owner_id.clone(),
            runtime_lease_token: session.runtime_lease_token.clone(),
        };
        if !lock(&self.queued).insert(key.clone()) {
            return None;
        }
        Some(ProviderRecoveryGuard {
            tracker: self.clone(),
            key: Some(key),
        })
    }
}

impl Drop for ProviderRecoveryGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            lock(&self.tracker.queued).remove(&key);
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
