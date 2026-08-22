use std::time::Duration;

use tokio::time::Instant;

const WINDOW: Duration = Duration::from_secs(10);
const MESSAGES_PER_WINDOW: usize = 256;
const BYTES_PER_WINDOW: usize = 2 * 1024 * 1024;
pub(crate) const CONTROL_FRAMES_PER_WINDOW: usize = 64;

pub(crate) struct IngressBudget {
    window_started: Instant,
    messages: usize,
    bytes: usize,
    control_frames: usize,
}

impl IngressBudget {
    pub(crate) fn new() -> Self {
        Self {
            window_started: Instant::now(),
            messages: 0,
            bytes: 0,
            control_frames: 0,
        }
    }

    pub(crate) fn admit(&mut self, bytes: usize, control_frame: bool) -> bool {
        let now = Instant::now();
        if now.duration_since(self.window_started) >= WINDOW {
            self.window_started = now;
            self.messages = 0;
            self.bytes = 0;
            self.control_frames = 0;
        }
        self.messages = self.messages.saturating_add(1);
        self.bytes = self.bytes.saturating_add(bytes);
        if control_frame {
            self.control_frames = self.control_frames.saturating_add(1);
        }
        self.messages <= MESSAGES_PER_WINDOW
            && self.bytes <= BYTES_PER_WINDOW
            && self.control_frames <= CONTROL_FRAMES_PER_WINDOW
    }
}
