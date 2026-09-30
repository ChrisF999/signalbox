//! Per-connection message rate limit (spec §8): more than
//! `MAX_MSGS_PER_S` messages in any one-second window closes the socket.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

pub const MAX_MSGS_PER_S: u32 = 20;
const WINDOW: Duration = Duration::from_secs(1);

/// A sliding window: the instants of the last `MAX_MSGS_PER_S` messages.
pub struct RateLimit {
    recent: VecDeque<Instant>,
}

impl RateLimit {
    pub fn new(_now: Instant) -> RateLimit {
        RateLimit { recent: VecDeque::with_capacity(MAX_MSGS_PER_S as usize) }
    }

    /// Count one message at `now`; `false` = it would be more than
    /// `MAX_MSGS_PER_S` within the last second.
    pub fn allow(&mut self, now: Instant) -> bool {
        if self.recent.len() == MAX_MSGS_PER_S as usize {
            // A message exactly a second old still counts (closed window).
            if now.saturating_duration_since(self.recent[0]) <= WINDOW {
                return false;
            }
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        true
    }
}
