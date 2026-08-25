use std::{
    collections::{HashMap, VecDeque},
    time::Duration,
};

use agentsassemble_persistence::PersistenceError;
use tokio::time::Instant;

const MUTATION_WINDOW: Duration = Duration::from_mins(1);
const MAX_MUTATIONS_PER_WINDOW: usize = 3_600;
const MAX_MUTATION_BYTES_PER_WINDOW: usize = 8 * 1024 * 1024;

pub(crate) struct ProviderWriteBudget {
    windows: HashMap<String, ProviderWindow>,
}

struct ProviderWindow {
    recent: VecDeque<(Instant, usize)>,
    bytes: usize,
}

impl ProviderWriteBudget {
    pub(crate) fn new() -> Self {
        Self {
            windows: HashMap::new(),
        }
    }

    pub(crate) fn admit(
        &mut self,
        provider_session_id: &str,
        payload_bytes: usize,
    ) -> Result<(), PersistenceError> {
        let now = Instant::now();
        let cutoff = now.checked_sub(MUTATION_WINDOW).unwrap_or(now);
        let window = self
            .windows
            .entry(provider_session_id.to_owned())
            .or_insert_with(|| ProviderWindow {
                recent: VecDeque::new(),
                bytes: 0,
            });
        while window.recent.front().is_some_and(|(at, _)| *at <= cutoff) {
            if let Some((_, expired_bytes)) = window.recent.pop_front() {
                window.bytes = window.bytes.saturating_sub(expired_bytes);
            }
        }
        let next_bytes = window.bytes.saturating_add(payload_bytes);
        if window.recent.len().saturating_add(1) > MAX_MUTATIONS_PER_WINDOW
            || next_bytes > MAX_MUTATION_BYTES_PER_WINDOW
        {
            return Err(PersistenceError::CommandRejected {
                code: "write_budget_exceeded",
                message: "Provider-session write budget exceeded.".to_owned(),
            });
        }
        window.bytes = next_bytes;
        window.recent.push_back((now, payload_bytes));
        Ok(())
    }
}
