use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

use agentsassemble_domain::AuthenticatedPrincipal;
use parking_lot::Mutex;

const WINDOW: Duration = Duration::from_secs(10);
const MAX_TRACKED_PRINCIPALS: usize = 512;
const MAX_TRACKED_ROOMS: usize = 512;

#[derive(Debug, Clone, Copy)]
struct ScopePolicy {
    messages: usize,
    bytes: usize,
    control_frames: usize,
    history_requests: usize,
    history_events: usize,
}

const GLOBAL_POLICY: ScopePolicy = ScopePolicy {
    messages: 4_096,
    bytes: 32 * 1024 * 1024,
    control_frames: 1_024,
    history_requests: 640,
    history_events: 6_400,
};
const PRINCIPAL_POLICY: ScopePolicy = ScopePolicy {
    messages: 256,
    bytes: 2 * 1024 * 1024,
    control_frames: 64,
    history_requests: 10,
    history_events: 1_000,
};
const ROOM_POLICY: ScopePolicy = ScopePolicy {
    messages: 2_048,
    bytes: 16 * 1024 * 1024,
    control_frames: 512,
    history_requests: 320,
    history_events: 3_200,
};

#[derive(Debug)]
struct WindowCounters {
    started_at: Instant,
    messages: usize,
    bytes: usize,
    control_frames: usize,
    history_requests: usize,
    history_events: usize,
}

impl WindowCounters {
    fn new(now: Instant) -> Self {
        Self {
            started_at: now,
            messages: 0,
            bytes: 0,
            control_frames: 0,
            history_requests: 0,
            history_events: 0,
        }
    }

    fn expired(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.started_at) >= WINDOW
    }

    fn charge_frame(
        &mut self,
        now: Instant,
        bytes: usize,
        control_frame: bool,
        policy: ScopePolicy,
    ) -> bool {
        self.prepare(now);
        self.messages = self.messages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if control_frame {
            self.control_frames = self.control_frames.saturating_add(1);
        }
        self.messages <= policy.messages
            && self.bytes <= policy.bytes
            && self.control_frames <= policy.control_frames
    }

    fn prepare(&mut self, now: Instant) {
        if self.expired(now) {
            *self = Self::new(now);
        }
    }

    fn admits_history(&self, events: usize, policy: ScopePolicy) -> bool {
        self.history_requests.saturating_add(1) <= policy.history_requests
            && self.history_events.saturating_add(events) <= policy.history_events
    }

    fn commit_history(&mut self, events: usize) {
        self.history_requests = self.history_requests.saturating_add(1);
        self.history_events = self.history_events.saturating_add(events);
    }
}

#[derive(Debug)]
struct SocketAdmissionState {
    global: WindowCounters,
    principals: HashMap<String, WindowCounters>,
    rooms: HashMap<String, WindowCounters>,
}

#[derive(Clone)]
pub(crate) struct SocketAdmission {
    state: Arc<Mutex<SocketAdmissionState>>,
}

impl SocketAdmission {
    pub(crate) fn new() -> Self {
        let now = Instant::now();
        Self {
            state: Arc::new(Mutex::new(SocketAdmissionState {
                global: WindowCounters::new(now),
                principals: HashMap::new(),
                rooms: HashMap::new(),
            })),
        }
    }

    pub(crate) fn admit_frame(
        &self,
        principal: &AuthenticatedPrincipal,
        bytes: usize,
        control_frame: bool,
    ) -> bool {
        self.admit_frame_at(principal, bytes, control_frame, Instant::now())
    }

    fn admit_frame_at(
        &self,
        principal: &AuthenticatedPrincipal,
        bytes: usize,
        control_frame: bool,
        now: Instant,
    ) -> bool {
        charge_frame_scopes(&mut self.state.lock(), principal, now, bytes, control_frame)
    }

    pub(crate) fn admit_history(
        &self,
        principal: &AuthenticatedPrincipal,
        requested_events: usize,
    ) -> bool {
        self.admit_history_at(principal, requested_events, Instant::now())
    }

    fn admit_history_at(
        &self,
        principal: &AuthenticatedPrincipal,
        requested_events: usize,
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
        let SocketAdmissionState {
            global,
            principals,
            rooms,
        } = &mut *state;
        let principal_window = principals
            .entry(principal.principal_id.clone())
            .or_insert_with(|| WindowCounters::new(now));
        let room_window = rooms
            .entry(principal.room_id.clone())
            .or_insert_with(|| WindowCounters::new(now));
        global.prepare(now);
        principal_window.prepare(now);
        room_window.prepare(now);
        if !global.admits_history(requested_events, GLOBAL_POLICY)
            || !principal_window.admits_history(requested_events, PRINCIPAL_POLICY)
            || !room_window.admits_history(requested_events, ROOM_POLICY)
        {
            return false;
        }
        global.commit_history(requested_events);
        principal_window.commit_history(requested_events);
        room_window.commit_history(requested_events);
        true
    }
}

fn charge_frame_scopes(
    state: &mut SocketAdmissionState,
    principal: &AuthenticatedPrincipal,
    now: Instant,
    bytes: usize,
    control_frame: bool,
) -> bool {
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
            .charge_frame(now, bytes, control_frame, GLOBAL_POLICY);
        charge_existing_frame(
            &mut state.principals,
            &principal.principal_id,
            now,
            bytes,
            control_frame,
            PRINCIPAL_POLICY,
        );
        charge_existing_frame(
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
        .charge_frame(now, bytes, control_frame, GLOBAL_POLICY);
    let principal_allowed = state
        .principals
        .entry(principal.principal_id.clone())
        .or_insert_with(|| WindowCounters::new(now))
        .charge_frame(now, bytes, control_frame, PRINCIPAL_POLICY);
    let room_allowed = state
        .rooms
        .entry(principal.room_id.clone())
        .or_insert_with(|| WindowCounters::new(now))
        .charge_frame(now, bytes, control_frame, ROOM_POLICY);
    global_allowed && principal_allowed && room_allowed
}

fn charge_existing_frame(
    windows: &mut HashMap<String, WindowCounters>,
    key: &str,
    now: Instant,
    bytes: usize,
    control_frame: bool,
    policy: ScopePolicy,
) {
    if let Some(window) = windows.get_mut(key) {
        window.charge_frame(now, bytes, control_frame, policy);
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

    use crate::room_channel::MAX_WS_MESSAGE_BYTES;

    use super::{GLOBAL_POLICY, MAX_TRACKED_ROOMS, PRINCIPAL_POLICY, ROOM_POLICY, SocketAdmission};

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

    fn fill_tracking_capacity(governor: &SocketAdmission, now: std::time::Instant) {
        for index in 0..MAX_TRACKED_ROOMS {
            assert!(governor.admit_frame_at(
                &principal_for(&format!("principal-{index}"), &format!("room-{index}")),
                1,
                false,
                now,
            ));
        }
    }

    #[test]
    fn principal_ingress_cannot_be_sharded_across_connections_or_rooms() {
        let governor = SocketAdmission::new();
        for index in 0..PRINCIPAL_POLICY.messages {
            let room = if index % 2 == 0 { "room-a" } else { "room-b" };
            assert!(governor.admit_frame(&principal(room), 0, false));
        }
        assert!(!governor.admit_frame(&principal("room-c"), 0, false));
    }

    #[test]
    fn rejected_raw_frames_remain_charged_until_window_reset() {
        let governor = SocketAdmission::new();
        let principal = principal("room");
        assert!(!governor.admit_frame(&principal, PRINCIPAL_POLICY.bytes + 1, false));
        assert!(!governor.admit_frame(&principal, 0, false));
    }

    #[test]
    fn control_frames_have_an_independent_principal_ceiling() {
        let governor = SocketAdmission::new();
        let principal = principal("room");
        for _ in 0..PRINCIPAL_POLICY.control_frames {
            assert!(governor.admit_frame(&principal, 0, true));
        }
        assert!(!governor.admit_frame(&principal, 0, true));
    }

    #[test]
    fn history_cost_cannot_be_sharded_across_connections_or_rooms() {
        let admission = SocketAdmission::new();
        let same_principal = principal("room-a");
        for _ in 0..PRINCIPAL_POLICY.history_events / 200 {
            assert!(admission.admit_history(&same_principal, 200));
        }
        assert!(!admission.admit_history(&principal("room-b"), 200));
        assert!(admission.admit_frame(&same_principal, 0, false));
    }

    #[test]
    fn room_history_cost_is_shared_across_principals() {
        let admission = SocketAdmission::new();
        for index in 0..ROOM_POLICY.history_events / 200 {
            assert!(admission.admit_history(
                &principal_for(&format!("principal-{index}"), "shared-room"),
                200,
            ));
        }
        assert!(
            !admission.admit_history(&principal_for("principal-over-limit", "shared-room"), 200,)
        );
    }

    #[test]
    fn small_pages_cannot_bypass_the_history_request_ceiling() {
        let admission = SocketAdmission::new();
        let principal = principal("room");
        for _ in 0..PRINCIPAL_POLICY.history_requests {
            assert!(admission.admit_history(&principal, 1));
        }
        assert!(!admission.admit_history(&principal, 1));
        assert!(admission.admit_frame(&principal, 0, false));
    }

    #[test]
    fn rejected_history_does_not_debit_broader_scopes() {
        let admission = SocketAdmission::new();
        let now = std::time::Instant::now();
        let blocked = principal_for("blocked", "room-a");
        for _ in 0..PRINCIPAL_POLICY.history_events / 200 {
            assert!(admission.admit_history_at(&blocked, 200, now));
        }
        for _ in 0..=(GLOBAL_POLICY.history_events / 200) {
            assert!(!admission.admit_history_at(&blocked, 200, now));
        }
        assert!(admission.admit_history_at(&principal_for("peer", "room-a"), 200, now));
        assert!(admission.admit_history_at(&principal_for("other", "room-b"), 200, now));
    }

    #[test]
    fn tracking_capacity_rejection_still_charges_the_global_scope() {
        let governor = SocketAdmission::new();
        let now = std::time::Instant::now();
        fill_tracking_capacity(&governor, now);
        let attempts = GLOBAL_POLICY.bytes / MAX_WS_MESSAGE_BYTES + 1;
        for index in 0..attempts {
            assert!(!governor.admit_frame_at(
                &principal_for(&format!("principal-{index}"), &format!("untracked-{index}")),
                MAX_WS_MESSAGE_BYTES,
                false,
                now,
            ));
        }

        assert!(!governor.admit_frame_at(
            &principal_for("principal-511", "room-511"),
            0,
            false,
            now,
        ));
    }

    #[test]
    fn tracking_capacity_rejection_still_charges_an_existing_principal_scope() {
        let governor = SocketAdmission::new();
        let now = std::time::Instant::now();
        fill_tracking_capacity(&governor, now);
        let attempts = PRINCIPAL_POLICY.bytes / MAX_WS_MESSAGE_BYTES + 1;
        for index in 0..attempts {
            assert!(!governor.admit_frame_at(
                &principal_for("principal-0", &format!("untracked-{index}")),
                MAX_WS_MESSAGE_BYTES,
                false,
                now,
            ));
        }

        assert!(!governor.admit_frame_at(&principal_for("principal-0", "room-0"), 0, false, now,));
    }
}
