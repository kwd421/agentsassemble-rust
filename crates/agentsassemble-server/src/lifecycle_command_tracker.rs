use std::{
    collections::HashSet,
    sync::{Arc, Mutex, MutexGuard},
};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LifecycleCommandKey {
    room_id: String,
    principal_id: String,
    request_id: String,
    action: String,
}

#[derive(Clone, Default)]
pub(crate) struct LifecycleCommandTracker {
    active: Arc<Mutex<HashSet<LifecycleCommandKey>>>,
}

pub(crate) struct LifecycleCommandGuard {
    tracker: LifecycleCommandTracker,
    key: Option<LifecycleCommandKey>,
}

impl LifecycleCommandTracker {
    pub(crate) fn try_claim(
        &self,
        room_id: &str,
        principal_id: &str,
        request_id: &str,
        action: &str,
    ) -> Option<LifecycleCommandGuard> {
        let key = lifecycle_action(action).then(|| LifecycleCommandKey {
            room_id: room_id.to_owned(),
            principal_id: principal_id.to_owned(),
            request_id: request_id.to_owned(),
            action: action.to_owned(),
        });
        if let Some(key) = &key
            && !lock(&self.active).insert(key.clone())
        {
            return None;
        }
        Some(LifecycleCommandGuard {
            tracker: self.clone(),
            key,
        })
    }
}

impl Drop for LifecycleCommandGuard {
    fn drop(&mut self) {
        if let Some(key) = self.key.take() {
            lock(&self.tracker.active).remove(&key);
        }
    }
}

fn lifecycle_action(action: &str) -> bool {
    matches!(
        action,
        "agent.create" | "agent.start" | "agent.resume" | "agent.stop"
    )
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::LifecycleCommandTracker;

    #[test]
    fn guard_exclusively_owns_one_exact_lifecycle_request() {
        let tracker = LifecycleCommandTracker::default();
        let guard = tracker
            .try_claim("room", "principal", "request", "agent.start")
            .unwrap_or_else(|| panic!("first exact owner was not admitted"));
        assert!(
            tracker
                .try_claim("room", "principal", "request", "agent.start")
                .is_none()
        );
        assert!(
            tracker
                .try_claim("room", "principal", "other", "agent.start")
                .is_some()
        );
        drop(guard);
        assert!(
            tracker
                .try_claim("room", "principal", "request", "agent.start")
                .is_some()
        );
    }
}
