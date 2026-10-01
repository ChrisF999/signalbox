//! The player's display settings (realism spec §4): signal aspects as the
//! real panel shows them (red/green) or as the driver sees them, the
//! headcode enquiry, and signal numbers. Kept per browser through a
//! `SettingsStore` the shell provides, as a few `key=value` lines.

use std::cell::RefCell;
use std::rc::Rc;

/// The key the web shell stores the settings under.
pub const SETTINGS_KEY: &str = "signalbox.settings";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AspectMode {
    /// Red for on, green for any proceed aspect, as on a real IECC.
    RedGreen,
    /// Red, yellow, double yellow, green.
    Real,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Settings {
    pub aspects: AspectMode,
    /// Clicking a headcode opens its enquiry window.
    pub enquiry: bool,
    /// Signal numbers beside every signal.
    pub numbers: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { aspects: AspectMode::RedGreen, enquiry: false, numbers: true }
    }
}

fn on_off(b: bool) -> &'static str {
    if b { "on" } else { "off" }
}

impl Settings {
    pub fn to_text(&self) -> String {
        let aspects = match self.aspects {
            AspectMode::RedGreen => "red_green",
            AspectMode::Real => "real",
        };
        format!("aspects={aspects}\nenquiry={}\nnumbers={}\n", on_off(self.enquiry), on_off(self.numbers))
    }

    /// Whatever was stored: unknown keys and bad values give the defaults,
    /// so nothing in the store can break the client.
    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            match (key.trim(), value.trim()) {
                ("aspects", "red_green") => s.aspects = AspectMode::RedGreen,
                ("aspects", "real") => s.aspects = AspectMode::Real,
                ("enquiry", "on") => s.enquiry = true,
                ("enquiry", "off") => s.enquiry = false,
                ("numbers", "on") => s.numbers = true,
                ("numbers", "off") => s.numbers = false,
                _ => {}
            }
        }
        s
    }
}

/// Where the settings live between visits: `localStorage` in the browser,
/// a file for the desktop client (D2).
pub trait SettingsStore {
    fn load(&self) -> Option<String>;
    fn save(&mut self, text: &str);
}

/// A store in memory; clones share it, so a test can read what was saved.
#[derive(Clone, Debug, Default)]
pub struct MemStore(Rc<RefCell<Option<String>>>);

impl MemStore {
    pub fn new() -> MemStore {
        MemStore::default()
    }

    pub fn text(&self) -> Option<String> {
        self.0.borrow().clone()
    }
}

impl SettingsStore for MemStore {
    fn load(&self) -> Option<String> {
        self.text()
    }

    fn save(&mut self, text: &str) {
        *self.0.borrow_mut() = Some(text.to_string());
    }
}
