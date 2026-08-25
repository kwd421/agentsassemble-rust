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
        let principal_capacity = ensure_capacity(
            &mut state.principals,
            &principal.principal_id,
            MAX_TRACKED_PRINCIPALS,
            now,
        );
        let room_capacity =
            ensure_capacity(&mut state.rooms, &principal.room_id, MAX_TRACKED_ROOMS, now);
        if !principal_capacity || !room_capacity {
            state
                .global
                .charge(now, bytes, control_frame, GLOBAL_POLICY);
            charge_existing(
                &mut state.principals,
                &principal.principal_id,
                now,
                bytes,
                control_frame,
                PRINCIPAL_POLICY,
            );
            charge_existing(
                &mut state.rooms,
                &principal.room_id,
                now,
                bytes,
                control_frame,
                ROOM_POLICY,
            );
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

fn charge_existing(
    windows: &mut HashMap<String, WindowCounters>,
    key: &str,
    now: Instant,
    bytes: usize,
    control_frame: bool,
    policy: ScopePolicy,
) {
    if let Some(window) = windows.get_mut(key) {
        window.charge(now, bytes, control_frame, policy);
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

    use crate::authenticated_channel::MAX_WS_WIRE_MESSAGE_BYTES;

    use super::{GLOBAL_POLICY, MAX_TRACKED_ROOMS, PRINCIPAL_POLICY, RawIngressGovernor};

    fn principal(room_id: &str) -> AuthenticatedPrincipal {
        principal_for("shared-human", room_id)
    }

    fn principal_for(principal_id: &str, room_id: &str) -> AuthenticatedPrincipal {
        AuthenticatedPrincipal {
            principal_id: principal_id.to_owned(),
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

    fn fill_tracking_capacity(governor: &RawIngressGovernor, now: std::time::Instant) {
        for index in 0..MAX_TRACKED_ROOMS {
            assert!(governor.admit_at(
                &principal_for(&format!("principal-{index}"), &format!("room-{index}")),
                1,
                false,
                now,
            ));
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

    #[test]
    fn tracking_capacity_rejection_still_charges_the_global_scope() {
        let governor = RawIngressGovernor::new();
        let now = std::time::Instant::now();
        fill_tracking_capacity(&governor, now);
        let attempts = GLOBAL_POLICY.bytes / MAX_WS_WIRE_MESSAGE_BYTES + 1;
        for index in 0..attempts {
            assert!(!governor.admit_at(
                &principal_for(&format!("principal-{index}"), &format!("untracked-{index}")),
                MAX_WS_WIRE_MESSAGE_BYTES,
                false,
                now,
            ));
        }

        assert!(!governor.admit_at(&principal_for("principal-511", "room-511"), 0, false, now,));
    }

    #[test]
    fn tracking_capacity_rejection_still_charges_an_existing_principal_scope() {
        let governor = RawIngressGovernor::new();
        let now = std::time::Instant::now();
        fill_tracking_capacity(&governor, now);
        let attempts = PRINCIPAL_POLICY.bytes / MAX_WS_WIRE_MESSAGE_BYTES + 1;
        for index in 0..attempts {
            assert!(!governor.admit_at(
                &principal_for("principal-0", &format!("untracked-{index}")),
                MAX_WS_WIRE_MESSAGE_BYTES,
                false,
                now,
            ));
        }

        assert!(!governor.admit_at(&principal_for("principal-0", "room-0"), 0, false, now,));
    }
}
