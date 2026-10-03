use std::{collections::HashMap, sync::Arc};

use agentsassemble_domain::AuthenticatedPrincipal;
use parking_lot::Mutex;
use thiserror::Error;

const MAX_CONNECTIONS: usize = 128;
const MAX_CONNECTIONS_PER_PRINCIPAL: usize = 8;
const MAX_CONNECTIONS_PER_ROOM: usize = 64;

#[derive(Debug, Clone, Copy)]
struct Limits {
    total: usize,
    principal: usize,
    room: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            total: MAX_CONNECTIONS,
            principal: MAX_CONNECTIONS_PER_PRINCIPAL,
            room: MAX_CONNECTIONS_PER_ROOM,
        }
    }
}

#[derive(Debug)]
struct ScopeEntry {
    generation: u64,
    active: usize,
}

#[derive(Debug)]
struct LeaseRecord {
    principal_id: String,
    principal_generation: u64,
    room_id: Option<String>,
    room_generation: u64,
}

#[derive(Debug, Default)]
struct AdmissionState {
    sequence: u64,
    active: usize,
    principals: HashMap<String, ScopeEntry>,
    rooms: HashMap<Option<String>, ScopeEntry>,
    leases: HashMap<u64, LeaseRecord>,
}

#[derive(Clone)]
pub(crate) struct ConnectionAdmission {
    limits: Limits,
    state: Arc<Mutex<AdmissionState>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub(crate) enum ConnectionAdmissionError {
    #[error("the process-wide WebSocket connection limit was reached")]
    GlobalLimit,
    #[error("the principal WebSocket connection limit was reached")]
    PrincipalLimit,
    #[error("the room WebSocket connection limit was reached")]
    RoomLimit,
    #[error("WebSocket connection lease identity is unavailable")]
    IdentityUnavailable,
}

pub(crate) struct ConnectionLease {
    owner: ConnectionAdmission,
    lease_id: u64,
}

impl ConnectionAdmission {
    pub(crate) fn new() -> Self {
        Self::with_limits(Limits::default())
    }

    fn with_limits(limits: Limits) -> Self {
        Self {
            limits,
            state: Arc::new(Mutex::new(AdmissionState::default())),
        }
    }

    pub(crate) fn acquire(
        &self,
        principal: &AuthenticatedPrincipal,
    ) -> Result<ConnectionLease, ConnectionAdmissionError> {
        self.acquire_scope(&principal.principal_id, Some(principal.room_id.clone()))
    }

    // Budget scope only: callers authenticate server ownership before admission.
    pub(crate) fn acquire_directory(&self) -> Result<ConnectionLease, ConnectionAdmissionError> {
        self.acquire_scope(agentsassemble_domain::LOCAL_OPERATOR_USER_ID, None)
    }

    fn acquire_scope(
        &self,
        principal_id: &str,
        room_id: Option<String>,
    ) -> Result<ConnectionLease, ConnectionAdmissionError> {
        let mut state = self.state.lock();
        if state.active >= self.limits.total {
            return Err(ConnectionAdmissionError::GlobalLimit);
        }
        if state
            .principals
            .get(principal_id)
            .is_some_and(|entry| entry.active >= self.limits.principal)
        {
            return Err(ConnectionAdmissionError::PrincipalLimit);
        }
        if state
            .rooms
            .get(&room_id)
            .is_some_and(|entry| entry.active >= self.limits.room)
        {
            return Err(ConnectionAdmissionError::RoomLimit);
        }

        let new_principal = !state.principals.contains_key(principal_id);
        let new_room = !state.rooms.contains_key(&room_id);
        let identities = 1_u64 + u64::from(new_principal) + u64::from(new_room);
        let final_sequence = state
            .sequence
            .checked_add(identities)
            .ok_or(ConnectionAdmissionError::IdentityUnavailable)?;
        let mut next_identity = state.sequence;
        let mut allocate_identity = || {
            next_identity = next_identity.saturating_add(1);
            next_identity
        };
        let principal_generation = if new_principal {
            allocate_identity()
        } else {
            let Some(entry) = state.principals.get(principal_id) else {
                return Err(ConnectionAdmissionError::IdentityUnavailable);
            };
            entry.generation
        };
        let room_generation = if new_room {
            allocate_identity()
        } else {
            let Some(entry) = state.rooms.get(&room_id) else {
                return Err(ConnectionAdmissionError::IdentityUnavailable);
            };
            entry.generation
        };
        let lease_id = allocate_identity();
        debug_assert_eq!(next_identity, final_sequence);

        let principal_entry =
            state
                .principals
                .entry(principal_id.to_owned())
                .or_insert(ScopeEntry {
                    generation: principal_generation,
                    active: 0,
                });
        principal_entry.active += 1;
        let room_entry = state.rooms.entry(room_id.clone()).or_insert(ScopeEntry {
            generation: room_generation,
            active: 0,
        });
        room_entry.active += 1;
        state.active += 1;
        state.sequence = final_sequence;
        state.leases.insert(
            lease_id,
            LeaseRecord {
                principal_id: principal_id.to_owned(),
                principal_generation,
                room_id,
                room_generation,
            },
        );
        Ok(ConnectionLease {
            owner: self.clone(),
            lease_id,
        })
    }

    fn release(&self, lease_id: u64) {
        let mut state = self.state.lock();
        let Some(record) = state.leases.remove(&lease_id) else {
            return;
        };
        state.active = state.active.saturating_sub(1);
        release_scope(
            &mut state.principals,
            &record.principal_id,
            record.principal_generation,
        );
        release_scope(&mut state.rooms, &record.room_id, record.room_generation);
    }
}

impl Drop for ConnectionLease {
    fn drop(&mut self) {
        self.owner.release(self.lease_id);
    }
}

fn release_scope<K: Eq + std::hash::Hash>(
    scopes: &mut HashMap<K, ScopeEntry>,
    key: &K,
    generation: u64,
) {
    let remove = scopes.get_mut(key).is_some_and(|entry| {
        if entry.generation != generation {
            return false;
        }
        entry.active = entry.active.saturating_sub(1);
        entry.active == 0
    });
    if remove {
        scopes.remove(key);
    }
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};

    use super::{ConnectionAdmission, ConnectionAdmissionError, Limits};

    fn principal(principal_id: &str, room_id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: principal_id.to_owned(),
            participant_id: format!("participant-{principal_id}"),
            display_name: principal_id.to_owned(),
            room_id: room_id.to_owned(),
            client_kind: ClientKind::Browser,
            invite_scope: InviteScope::ReadWrite,
            is_operator: false,
            capabilities: CapabilitySet::for_principal(
                ClientKind::Browser,
                InviteScope::ReadWrite,
                false,
            ),
        }
    }

    #[test]
    fn directory_budget_does_not_charge_a_named_product_room() {
        let admission = ConnectionAdmission::with_limits(Limits {
            total: 2,
            principal: 2,
            room: 1,
        });
        let directory = admission
            .acquire_directory()
            .unwrap_or_else(|e| panic!("directory lease: {e}"));
        let room = admission
            .acquire(&principal("guest", "server-directory"))
            .unwrap_or_else(|e| panic!("directory charged a product room: {e}"));
        drop((directory, room));
    }

    #[test]
    fn rejected_scope_does_not_charge_another_scope() {
        let admission = ConnectionAdmission::with_limits(Limits {
            total: 2,
            principal: 1,
            room: 1,
        });
        let first = admission
            .acquire(&principal("a", "room-a"))
            .unwrap_or_else(|error| panic!("first lease: {error}"));
        assert!(matches!(
            admission.acquire(&principal("a", "room-b")),
            Err(ConnectionAdmissionError::PrincipalLimit)
        ));
        let second = admission
            .acquire(&principal("b", "room-b"))
            .unwrap_or_else(|error| panic!("failed admission charged room/global: {error}"));
        assert!(matches!(
            admission.acquire(&principal("c", "room-c")),
            Err(ConnectionAdmissionError::GlobalLimit)
        ));
        drop((first, second));
    }

    #[test]
    fn stale_lease_release_cannot_free_a_replacement_generation() {
        let admission = ConnectionAdmission::with_limits(Limits {
            total: 1,
            principal: 1,
            room: 1,
        });
        let old = admission
            .acquire(&principal("a", "room"))
            .unwrap_or_else(|error| panic!("old lease: {error}"));
        let old_id = old.lease_id;
        drop(old);
        let replacement = admission
            .acquire(&principal("a", "room"))
            .unwrap_or_else(|error| panic!("replacement lease: {error}"));
        admission.release(old_id);
        assert!(matches!(
            admission.acquire(&principal("b", "other")),
            Err(ConnectionAdmissionError::GlobalLimit)
        ));
        drop(replacement);
    }
}
