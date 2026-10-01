//! The display settings and the tutorial ticks in the browser's
//! `localStorage` (realism spec §4, tutorial spec §3): remembered per
//! browser, each under its own key. Without storage (private windows,
//! blocked cookies) the defaults simply apply.

use client_core::SettingsStore;

pub struct LocalStore {
    key: &'static str,
}

impl LocalStore {
    pub fn new(key: &'static str) -> LocalStore {
        LocalStore { key }
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl SettingsStore for LocalStore {
    fn load(&self) -> Option<String> {
        storage()?.get_item(self.key).ok().flatten()
    }

    fn save(&mut self, text: &str) {
        if let Some(s) = storage() {
            // A full or refused store just means it is not kept.
            let _ = s.set_item(self.key, text);
        }
    }
}
