use std::{collections::HashMap, hash::Hash, net::IpAddr, num::NonZeroU32};

use governor::{
    Quota, RateLimiter,
    clock::{Clock, DefaultClock},
    middleware::NoOpMiddleware,
    state::{InMemoryState, NotKeyed},
};
use parking_lot::Mutex;

type Direct<C> = RateLimiter<NotKeyed, InMemoryState, C, NoOpMiddleware<<C as Clock>::Instant>>;
const MAX_KEYS: usize = 512;
const GLOBAL: NonZeroU32 = NonZeroU32::new(256).expect("positive recovery quota");
const NETWORK: NonZeroU32 = NonZeroU32::new(16).expect("positive recovery quota");
const CODE: NonZeroU32 = NonZeroU32::new(8).expect("positive recovery quota");

/// The retained recovery route has global, network and code attempt budgets.
/// Governor owns replenishment; this owner bounds attacker-selected keys. No
/// waiting task, timestamp mirror, or periodic housekeeping is introduced.
pub(crate) struct GuestRecoveryAttempts<C: Clock = DefaultClock> {
    global: Direct<C>,
    networks: Bounded<IpAddr, C>,
    codes: Bounded<[u8; 32], C>,
}

impl Default for GuestRecoveryAttempts {
    fn default() -> Self {
        Self::with_clock(DefaultClock::default())
    }
}

impl<C: Clock + Clone> GuestRecoveryAttempts<C> {
    fn with_clock(clock: C) -> Self {
        Self {
            global: RateLimiter::direct_with_clock(Quota::per_minute(GLOBAL), clock.clone()),
            networks: Bounded::new(NETWORK, clock.clone()),
            codes: Bounded::new(CODE, clock),
        }
    }

    pub(crate) fn allows(&self, network: IpAddr, code: [u8; 32]) -> bool {
        self.global.check().is_ok() && self.networks.allows(network) && self.codes.allows(code)
    }
}

struct Bounded<K, C: Clock> {
    burst: NonZeroU32,
    clock: C,
    entries: Mutex<HashMap<K, Direct<C>>>,
}

impl<K: Eq + Hash, C: Clock + Clone> Bounded<K, C> {
    fn new(burst: NonZeroU32, clock: C) -> Self {
        Self {
            burst,
            clock,
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn allows(&self, key: K) -> bool {
        let mut entries = self.entries.lock();
        if let Some(limiter) = entries.get(&key) {
            return limiter.check().is_ok();
        }
        if entries.len() >= MAX_KEYS {
            // A fully replenished cell is equivalent to absent state. The
            // successful full-burst check consumes only a cell immediately dropped.
            entries.retain(|_, limiter| !matches!(limiter.check_n(self.burst), Ok(Ok(()))));
        }
        if entries.len() >= MAX_KEYS {
            return false;
        }
        let limiter =
            RateLimiter::direct_with_clock(Quota::per_minute(self.burst), self.clock.clone());
        let allowed = limiter.check().is_ok();
        entries.insert(key, limiter);
        allowed
    }
}

#[cfg(test)]
mod tests {
    use super::{Bounded, CODE, GuestRecoveryAttempts, MAX_KEYS};
    use governor::clock::FakeRelativeClock;
    use std::{net::IpAddr, time::Duration};

    #[test]
    fn budgets_are_shared_by_network_and_code_and_replenish_without_sleeping() {
        let clock = FakeRelativeClock::default();
        let limits = GuestRecoveryAttempts::with_clock(clock.clone());
        let network = IpAddr::from([192, 0, 2, 1]);
        for _ in 0..8 {
            assert!(limits.allows(network, [1; 32]));
        }
        assert!(!limits.allows(network, [1; 32]));
        for value in 2..=8 {
            assert!(limits.allows(network, [value; 32]));
        }
        assert!(!limits.allows(network, [9; 32]));
        clock.advance(Duration::from_mins(1));
        assert!(limits.allows(network, [1; 32]));
    }

    #[test]
    fn key_capacity_is_bounded_and_only_full_cells_can_be_retired() {
        let clock = FakeRelativeClock::default();
        let limits = Bounded::new(CODE, clock.clone());
        for key in 0..MAX_KEYS {
            assert!(limits.allows(key));
        }
        assert!(!limits.allows(MAX_KEYS));
        assert_eq!(limits.entries.lock().len(), MAX_KEYS);
        clock.advance(Duration::from_mins(1));
        assert!(limits.allows(MAX_KEYS));
        assert_eq!(limits.entries.lock().len(), 1);
    }
}
