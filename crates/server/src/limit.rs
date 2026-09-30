//! Per-connection message rate limit (spec §8): more than
//! `MAX_MSGS_PER_S` messages in one window of a second closes the socket.

use std::time::{Duration, Instant};

pub const MAX_MSGS_PER_S: u32 = 20;
const WINDOW: Duration = Duration::from_secs(1);

pub struct RateLimit {
    window_start: Instant,
    count: u32,
}

impl RateLimit {
    pub fn new(now: Instant) -> RateLimit {
        RateLimit { window_start: now, count: 0 }
    }

    /// Count one message at `now`; `false` = over the limit.
    pub fn allow(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.window_start) >= WINDOW {
            self.window_start = now;
            self.count = 0;
        }
        self.count += 1;
        self.count <= MAX_MSGS_PER_S
    }
}
