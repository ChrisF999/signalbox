//! Display settings (realism spec §4): defaults, and what a store keeps.

use client_core::{AspectMode, MemStore, Settings, SettingsStore};

#[test]
fn the_defaults_are_the_real_panel() {
    let d = Settings::default();
    assert_eq!((d.aspects, d.enquiry, d.numbers), (AspectMode::RedGreen, false, true));
}

#[test]
fn settings_survive_their_text() {
    let all = [AspectMode::RedGreen, AspectMode::Real];
    for aspects in all {
        for enquiry in [false, true] {
            for numbers in [false, true] {
                let s = Settings { aspects, enquiry, numbers };
                assert_eq!(Settings::from_text(&s.to_text()), s, "{}", s.to_text());
            }
        }
    }
    assert_eq!(Settings { aspects: AspectMode::Real, enquiry: true, numbers: false }.to_text(), "aspects=real\nenquiry=on\nnumbers=off\n");
}

#[test]
fn anything_else_in_the_store_gives_the_defaults() {
    for junk in ["", "garbage", "aspects=purple\nnumbers=maybe", "=\n==\n", "{\"aspects\": \"real\"}", "\u{0}\u{ffff}"] {
        assert_eq!(Settings::from_text(junk), Settings::default(), "{junk:?}");
    }
    let partial = Settings::from_text(" enquiry = on \nwho=knows\n");
    assert_eq!(partial, Settings { enquiry: true, ..Settings::default() }, "good lines count, the rest is ignored");
}

#[test]
fn a_mem_store_is_shared_by_its_clones() {
    let store = MemStore::new();
    let mut writer = store.clone();
    assert_eq!(store.load(), None);
    writer.save("numbers=off\n");
    assert_eq!(store.text().as_deref(), Some("numbers=off\n"));
    assert_eq!(client_core::settings::SETTINGS_KEY, "signalbox.settings");
}
