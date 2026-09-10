//! The packaged runtime's nested lifetime deadlines. Consumers use these same
//! leaf limits; the desktop must outlive its own supervisor's cleanup work.
use std::time::Duration;

pub const PROVIDER_FILESYSTEM_TIMEOUT: Duration = Duration::from_secs(10);
pub const PROVIDER_CONTROL_TIMEOUT: Duration = Duration::from_mins(1);
pub const PROVIDER_DRIVER_STOP_TIMEOUT: Duration = Duration::from_secs(5);
pub const ATTENDEE_HTTP_TIMEOUT: Duration = Duration::from_secs(30);
pub const ATTENDEE_REMOTE_CLEANUP_TIMEOUT: Duration = Duration::from_secs(30);
pub const CONNECTION_DRAIN_TIMEOUT: Duration = Duration::from_secs(6);
pub const ROOM_DRAIN_TIMEOUT: Duration = Duration::from_secs(3);
pub const PROVIDER_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

// A pending reservation can validate workspace and executable, join the factory
// handshake, regain the driver, receive Stop, and then join the worker's exit.
// Runtime slots drain concurrently, so this allowance is not multiplied by count.
pub const PROVIDER_DRAIN_GRACE: Duration = Duration::from_secs(
    2 * PROVIDER_FILESYSTEM_TIMEOUT.as_secs()
        + 3 * PROVIDER_CONTROL_TIMEOUT.as_secs()
        + PROVIDER_DRIVER_STOP_TIMEOUT.as_secs(),
);

// Cancel either resolves the in-flight admission plus one exact retry, or joins
// an already-dispatched start and stop. It cannot start after cancelled admission.
// Remote departure/cleanup begins only after that local custody is resolved.
pub const ATTENDEE_DRAIN_GRACE: Duration = Duration::from_secs(
    max_seconds(
        2 * ATTENDEE_HTTP_TIMEOUT.as_secs(),
        PROVIDER_DRAIN_GRACE.as_secs(),
    ) + ATTENDEE_REMOTE_CLEANUP_TIMEOUT.as_secs(),
);

// After attendees: connections, the current reconciliation effect (including its
// observation batch), room owners, concurrent managed runtimes, and catalog probes.
// Login/update/usage and ingress drain alongside this last chain. OS/filesystem or
// storage stalls are not completion proofs: this remains a finite emergency
// envelope, not a timeout that drops an individual cleanup future and reports OK.
pub const RECONCILIATION_OBSERVATION_TIMEOUT: Duration = Duration::from_secs(2);
pub const RECONCILIATION_OBSERVATION_CONCURRENCY: usize = 8;
pub const RECONCILIATION_PAGE_SIZE: u8 = 64;
pub const SERVER_SHUTDOWN_GRACE: Duration = Duration::from_secs(
    ATTENDEE_DRAIN_GRACE.as_secs()
        + CONNECTION_DRAIN_TIMEOUT.as_secs()
        + PROVIDER_DRAIN_GRACE.as_secs()
        + RECONCILIATION_OBSERVATION_TIMEOUT.as_secs()
            * (RECONCILIATION_PAGE_SIZE as usize).div_ceil(RECONCILIATION_OBSERVATION_CONCURRENCY)
                as u64
        + ROOM_DRAIN_TIMEOUT.as_secs()
        + PROVIDER_DRAIN_GRACE.as_secs()
        + PROVIDER_PROBE_TIMEOUT.as_secs(),
);

// Give the guardian its complete server interval and time to perform its own
// process-tree termination before the desktop's last-resort escalation.
pub const SUPERVISOR_REAP_GRACE: Duration = Duration::from_secs(3);
pub const DESKTOP_SHUTDOWN_GRACE: Duration =
    SERVER_SHUTDOWN_GRACE.saturating_add(SUPERVISOR_REAP_GRACE);

const fn max_seconds(left: u64, right: u64) -> u64 {
    if left > right { left } else { right }
}
