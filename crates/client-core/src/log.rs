//! The alarms and event log: a bounded list of lines, newest last.

use std::collections::VecDeque;

/// Lines kept; older ones fall off.
pub const LOG_CAP: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    /// Sim time when it arrived, if a view was held.
    pub sim_time: Option<f64>,
    pub text: String,
    pub alarm: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Log {
    entries: VecDeque<LogEntry>,
}

impl Log {
    pub fn push(&mut self, sim_time: Option<f64>, text: String, alarm: bool) {
        if self.entries.len() == LOG_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(LogEntry { sim_time, text, alarm });
    }

    /// Oldest first.
    pub fn entries(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
