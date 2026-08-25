use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};

use agentsassemble_persistence::PersistenceError;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

const MUTATION_WINDOW: Duration = Duration::from_mins(1);
const MAX_MUTATIONS_PER_WINDOW: usize = 3_600;
const MAX_MUTATION_BYTES_PER_WINDOW: usize = 8 * 1024 * 1024;
const MAX_TRACKED_PRINCIPALS: usize = 512;
const MAX_TRACKED_MUTATIONS: usize = 32_768;
const MAX_INFLIGHT_MUTATIONS: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MutationIdentity([u8; 32]);

impl MutationIdentity {
    pub(crate) fn new(room_id: &str, request_id: &str, action: &str, payload_hash: &str) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(b"agentsassemble.principal-mutation.v1\0");
        for field in [room_id, request_id, action, payload_hash] {
            hasher.update((field.len() as u64).to_be_bytes());
            hasher.update(field.as_bytes());
        }
        Self(hasher.finalize().into())
    }
}

#[derive(Debug)]
struct PrincipalWindow {
    recent: VecDeque<MutationCharge>,
    bytes: usize,
    retryable: HashMap<MutationIdentity, u64>,
}

#[derive(Debug)]
struct MutationCharge {
    at: Instant,
    bytes: usize,
    identity: MutationIdentity,
    charge_id: u64,
}

impl PrincipalWindow {
    fn new() -> Self {
        Self {
            recent: VecDeque::new(),
            bytes: 0,
            retryable: HashMap::new(),
        }
    }

    fn prune(&mut self, now: Instant) -> usize {
        let before = self.recent.len();
        let cutoff = now.checked_sub(MUTATION_WINDOW).unwrap_or(now);
        while self
            .recent
            .front()
            .is_some_and(|charge| charge.at <= cutoff)
        {
            if let Some(expired) = self.recent.pop_front() {
                self.bytes = self.bytes.saturating_sub(expired.bytes);
                if self.retryable.get(&expired.identity) == Some(&expired.charge_id) {
                    self.retryable.remove(&expired.identity);
                }
            }
        }
        before.saturating_sub(self.recent.len())
    }

    fn charge(
        &mut self,
        now: Instant,
        identity: MutationIdentity,
        payload_bytes: usize,
        charge_id: u64,
    ) -> Result<(), PersistenceError> {
        let next_bytes = self.bytes.saturating_add(payload_bytes);
        if self.recent.len().saturating_add(1) > MAX_MUTATIONS_PER_WINDOW
            || next_bytes > MAX_MUTATION_BYTES_PER_WINDOW
        {
            return Err(budget_exceeded());
        }
        self.bytes = next_bytes;
        self.retryable.insert(identity, charge_id);
        self.recent.push_back(MutationCharge {
            at: now,
            bytes: payload_bytes,
            identity,
            charge_id,
        });
        Ok(())
    }
}

#[derive(Debug, Default)]
struct MutationState {
    sequence: u64,
    windows: HashMap<String, PrincipalWindow>,
    tracked_mutations: usize,
}

impl MutationState {
    fn prune_principal(&mut self, principal_id: &str, now: Instant) {
        let removed = self
            .windows
            .get_mut(principal_id)
            .map_or(0, |window| window.prune(now));
        self.tracked_mutations = self.tracked_mutations.saturating_sub(removed);
    }

    fn prune_all(&mut self, now: Instant) {
        let mut removed = 0;
        self.windows.retain(|_, window| {
            removed += window.prune(now);
            !window.recent.is_empty()
        });
        self.tracked_mutations = self.tracked_mutations.saturating_sub(removed);
    }

    fn charge(
        &mut self,
        principal_id: &str,
        identity: MutationIdentity,
        payload_bytes: usize,
        now: Instant,
    ) -> Result<u64, PersistenceError> {
        self.prune_principal(principal_id, now);
        if let Some(charge_id) = self
            .windows
            .get(principal_id)
            .and_then(|window| window.retryable.get(&identity))
        {
            return Ok(*charge_id);
        }
        if (!self.windows.contains_key(principal_id)
            && self.windows.len() >= MAX_TRACKED_PRINCIPALS)
            || self.tracked_mutations >= MAX_TRACKED_MUTATIONS
        {
            self.prune_all(now);
        }
        if !self.windows.contains_key(principal_id) && self.windows.len() >= MAX_TRACKED_PRINCIPALS
        {
            return Err(capacity_exceeded());
        }
        if self.tracked_mutations >= MAX_TRACKED_MUTATIONS {
            return Err(capacity_exceeded());
        }
        let charge_id = self.sequence.checked_add(1).ok_or_else(capacity_exceeded)?;
        self.windows
            .entry(principal_id.to_owned())
            .or_insert_with(PrincipalWindow::new)
            .charge(now, identity, payload_bytes, charge_id)?;
        self.sequence = charge_id;
        self.tracked_mutations += 1;
        Ok(charge_id)
    }

    fn resolve(&mut self, principal_id: &str, identity: MutationIdentity, charge_id: u64) {
        let Some(window) = self.windows.get_mut(principal_id) else {
            return;
        };
        if window.retryable.get(&identity) == Some(&charge_id) {
            window.retryable.remove(&identity);
        }
    }
}

#[derive(Clone)]
pub(crate) struct PrincipalMutationAdmission {
    state: Arc<Mutex<MutationState>>,
    inflight: Arc<Semaphore>,
}

pub(crate) struct MutationDebit {
    owner: PrincipalMutationAdmission,
    principal_id: String,
    identity: MutationIdentity,
    charge_id: u64,
}

impl MutationDebit {
    pub(crate) fn resolve(&self) {
        self.owner
            .state
            .lock()
            .resolve(&self.principal_id, self.identity, self.charge_id);
    }
}

impl PrincipalMutationAdmission {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(MutationState::default())),
            inflight: Arc::new(Semaphore::new(MAX_INFLIGHT_MUTATIONS)),
        }
    }

    pub(crate) fn charge(
        &self,
        principal_id: &str,
        identity: MutationIdentity,
        payload_bytes: usize,
    ) -> Result<MutationDebit, PersistenceError> {
        self.charge_at(principal_id, identity, payload_bytes, Instant::now())
    }

    fn charge_at(
        &self,
        principal_id: &str,
        identity: MutationIdentity,
        payload_bytes: usize,
        now: Instant,
    ) -> Result<MutationDebit, PersistenceError> {
        let charge_id = self
            .state
            .lock()
            .charge(principal_id, identity, payload_bytes, now)?;
        Ok(MutationDebit {
            owner: self.clone(),
            principal_id: principal_id.to_owned(),
            identity,
            charge_id,
        })
    }

    pub(crate) fn acquire_inflight(&self) -> Result<OwnedSemaphorePermit, PersistenceError> {
        self.inflight
            .clone()
            .try_acquire_owned()
            .map_err(|_| PersistenceError::CommandUnresolved {
                code: "write_inflight_limited",
                message: "Authenticated write concurrency is temporarily full.".to_owned(),
            })
    }
}

fn budget_exceeded() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "write_budget_exceeded",
        message: "Authenticated principal write budget exceeded.".to_owned(),
    }
}

fn capacity_exceeded() -> PersistenceError {
    PersistenceError::CommandRejected {
        code: "write_budget_capacity_exceeded",
        message: "Authenticated write admission capacity is unavailable.".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_persistence::PersistenceError;

    use super::{MutationIdentity, PrincipalMutationAdmission};

    fn identity(payload_hash: &str) -> MutationIdentity {
        MutationIdentity::new("room", "request", "message.send", payload_hash)
    }

    #[test]
    fn exact_unresolved_retry_reuses_one_permanent_debit() {
        let admission = PrincipalMutationAdmission::new();
        admission
            .charge("principal", identity("one"), 8 * 1024 * 1024)
            .unwrap_or_else(|error| panic!("first debit: {error}"));
        admission
            .charge("principal", identity("one"), 8 * 1024 * 1024)
            .unwrap_or_else(|error| panic!("same intent must not debit twice: {error}"));
        let Err(error) = admission.charge("principal", identity("changed"), 1) else {
            panic!("a changed intent must consume another debit");
        };
        assert!(matches!(
            error,
            PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                ..
            }
        ));
    }

    #[test]
    fn definitive_resolution_closes_retry_exemption_without_refunding_debit() {
        let admission = PrincipalMutationAdmission::new();
        let debit = admission
            .charge("principal", identity("one"), 8 * 1024 * 1024)
            .unwrap_or_else(|error| panic!("first debit: {error}"));
        debit.resolve();
        let Err(error) = admission.charge("principal", identity("one"), 1) else {
            panic!("a resolved request must consume a new non-refunded debit");
        };
        assert!(matches!(
            error,
            PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                ..
            }
        ));
    }
}
