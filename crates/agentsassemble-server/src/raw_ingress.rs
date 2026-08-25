use std::{collections::HashMap, sync::Arc, time::Duration};

use agentsassemble_domain::AuthenticatedPrincipal;
use parking_lot::Mutex;
use std::time::Instant;

const WINDOW: Duration = Duration::from_secs(10);
const MAX_TRACKED_PRINCIPALS: usize = 512;
const MAX_TRACKED_ROOMS: usize = 512;

#[derive(Debug, Clone, Copy)]
struct ScopePolicy {
    messages: usize,
    bytes: usize,
    control_frames: usize,
}

const GLOBAL_POLICY: ScopePolicy = ScopePolicy {
    messages: 4_096,
    bytes: 32 * 1024 * 1024,
    control_frames: 1_024,
};
const PRINCIPAL_POLICY: ScopePolicy = ScopePolicy {
    messages: 256,
    bytes: 2 * 1024 * 1024,
    control_frames: 64,
};
const ROOM_POLICY: ScopePolicy = ScopePolicy {
    messages: 2_048,
    bytes: 16 * 1024 * 1024,
    control_frames: 512,
};

#[derive(Debug)]
struct WindowCounters {
    started_at: Instant,
    messages: usize,
    bytes: usize,
    control_frames: usize,
}

impl WindowCounters {
    fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            messages: 0,
            bytes: 0,
            control_frames: 0,
        }
    }

    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= WINDOW
    }

    fn charge(
        &mut self,
        now: Instant,
        bytes: usize,
        control_frame: bool,
        policy: ScopePolicy,
    ) -> bool {
        if self.expired(now) {
            *self = Self::new(now);
        }
        self.messages = self.messages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if control_frame {
            self.control_frames = self.control_frames.saturating_add(1);
        }
        self.messages <= policy.messages
            && self.bytes <= policy.bytes
            && self.control_frames <= policy.control_frames
    }
}

#[derive(Debug)]
struct RawIngressState {
    global: WindowCounters,
    principals: HashMap<String, WindowCounters>,
    rooms: HashMap<String, WindowCounters>,
}

#[derive(Clone)]
pub(crate) struct RawIngressGovernor {
    state: Arc<Mutex<RawIngressState>>,
}

impl RawIngressGovernor {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(RawIngressState {
                global: WindowCounters::new(now),
                principals: HashMap::new(),
                rooms: HashMap::new(),
            })),
        }
    }

    pub(crate) fn admit(
        &self,
        principal: &AuthenticatedPrincipal,
        bytes: usize,
        control_frame: bool,
    ) -> bool {
        self.admit_at(principal, bytes, control_frame, Instant::now())
    }

    fn admit_at(
        &self,
        principal: &AuthenticatedPrincipal,
        bytes: usize,
        control_frame: bool,
        now: Instant,
    ) -> bool {
        let mut state = self.state.lock();
        if !ensure_capacity(
            &mut state.principals,
            &principal.principal_id,
            MAX_TRACKED_PRINCIPALS,
            now,
        ) || !ensure_capacity(&mut state.rooms, &principal.room_id, MAX_TRACKED_ROOMS, now)
        {
            return false;
        }

        let global_allowed = state
            .global
            .charge(now, bytes, control_frame, GLOBAL_POLICY);
        let principal_allowed = state
            .principals
            .entry(principal.principal_id.clone())
            .or_insert_with(|| WindowCounters::new(now))
            .charge(now, bytes, control_frame, PRINCIPAL_POLICY);
        let room_allowed = state
            .rooms
            .entry(principal.room_id.clone())
            .or_insert_with(|| WindowCounters::new(now))
            .charge(now, bytes, control_frame, ROOM_POLICY);
        global_allowed && principal_allowed && room_allowed
    }
}

fn ensure_capacity(
    windows: &mut HashMap<String, WindowCounters>,
    key: &str,
    capacity: usize,
    now: Instant,
) -> bool {
    if windows.contains_key(key) {
        return true;
    }
    if windows.len() >= capacity {
        windows.retain(|_, window| !window.expired(now));
    }
    if windows.len() >= capacity {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use agentsassemble_domain::{AuthenticatedPrincipal, CapabilitySet, ClientKind, InviteScope};

    use super::{PRINCIPAL_POLICY, RawIngressGovernor};

    fn principal(room_id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: "shared-human".to_owned(),
            participant_id: format!("participant-{room_id}"),
            display_name: "Human".to_owned(),
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
    fn principal_ingress_cannot_be_sharded_across_connections_or_rooms() {
        let governor = RawIngressGovernor::new();
        for index in 0..PRINCIPAL_POLICY.messages {
            let room = if index % 2 == 0 { "room-a" } else { "room-b" };
            assert!(governor.admit(&principal(room), 0, false));
        }
        assert!(!governor.admit(&principal("room-c"), 0, false));
    }

    #[test]
    fn rejected_raw_frames_remain_charged_until_window_reset() {
        let governor = RawIngressGovernor::new();
        let principal = principal("room");
        assert!(!governor.admit(&principal, PRINCIPAL_POLICY.bytes + 1, false));
        assert!(!governor.admit(&principal, 0, false));
    }

    #[test]
    fn control_frames_have_an_independent_principal_ceiling() {
        let governor = RawIngressGovernor::new();
        let principal = principal("room");
        for _ in 0..PRINCIPAL_POLICY.control_frames {
            assert!(governor.admit(&principal, 0, true));
        }
        assert!(!governor.admit(&principal, 0, true));
    }
}
