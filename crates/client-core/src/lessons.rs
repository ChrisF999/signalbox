//! Which tutorial lessons this browser has completed (tutorial spec §3):
//! the lobby ticks them. Kept through a `SettingsStore` like the settings,
//! as one lesson id per line; anything else in the store is ignored.

use std::collections::BTreeSet;

/// The key the web shell stores the ticks under.
pub const LESSONS_KEY: &str = "signalbox.lessons";

/// A lesson id as the front sends it: 1–40 of `a-z`, `0-9`, `-`.
fn valid_id(s: &str) -> bool {
    (1..=40).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LessonTicks(BTreeSet<String>);

impl LessonTicks {
    /// Whatever was stored: lines that are not lesson ids are dropped.
    pub fn from_text(text: &str) -> LessonTicks {
        LessonTicks(text.lines().map(str::trim).filter(|l| valid_id(l)).map(str::to_string).collect())
    }

    pub fn to_text(&self) -> String {
        self.0.iter().map(|id| format!("{id}\n")).collect()
    }

    pub fn done(&self, id: &str) -> bool {
        self.0.contains(id)
    }

    /// Tick `id`; `false` if it already was, or is not a lesson id.
    pub fn insert(&mut self, id: &str) -> bool {
        valid_id(id) && self.0.insert(id.to_string())
    }
}
