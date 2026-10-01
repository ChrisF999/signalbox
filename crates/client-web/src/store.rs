//! The display settings in the browser's `localStorage` (realism spec §4):
//! remembered per browser. Without storage (private windows, blocked
//! cookies) the defaults simply apply.

use client_core::SettingsStore;
use client_core::settings::SETTINGS_KEY;

pub struct LocalStore;

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

impl SettingsStore for LocalStore {
    fn load(&self) -> Option<String> {
        storage()?.get_item(SETTINGS_KEY).ok().flatten()
    }

    fn save(&mut self, text: &str) {
        if let Some(s) = storage() {
            // A full or refused store just means the setting is not kept.
            let _ = s.set_item(SETTINGS_KEY, text);
        }
    }
}
